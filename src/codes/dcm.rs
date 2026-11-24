#![allow(non_upper_case_globals)]
use super::Code;
pub const SCHEME: &str = "DCM";
const fn c(value: &'static str, meaning: &'static str) -> Code<'static> {
    Code::new(value, SCHEME, meaning)
}
pub const AAPM204APDimension: Code<'static> = c("113935", "AAPM 204 AP Dimension");
pub const AAPM204EffectiveDiameterEstimatedFromPatientAge: Code<'static> = c("113937", "AAPM 204 Effective Diameter Estimated From Patient Age");
pub const AAPM204LateralDimension: Code<'static> = c("113934", "AAPM 204 Lateral Dimension");
pub const AAPM204SumOfLateralAndAPDimension: Code<'static> = c("113936", "AAPM 204 Sum of Lateral and AP Dimension");
pub const AAPM220: Code<'static> = c("113987", "AAPM 220");
pub const AAPMTG43: Code<'static> = c("131323", "AAPM TG-43");
pub const ACDConstant: Code<'static> = c("111768", "ACD Constant");
pub const ACRAccreditationPhantomCT: Code<'static> = c("113682", "ACR Accreditation Phantom - CT");
pub const ACRAccreditationPhantomECT: Code<'static> = c("113686", "ACR Accreditation Phantom - ECT");
pub const ACRAccreditationPhantomECTPerPET: Code<'static> = c("113688", "ACR Accreditation Phantom - ECT/PET");
pub const ACRAccreditationPhantomMR: Code<'static> = c("113683", "ACR Accreditation Phantom - MR");
pub const ACRAccreditationPhantomMammography: Code<'static> = c("113684", "ACR Accreditation Phantom - Mammography");
pub const ACRAccreditationPhantomPET: Code<'static> = c("113687", "ACR Accreditation Phantom - PET");
pub const ACRAccreditationPhantomPETFaceplate: Code<'static> = c("113689", "ACR Accreditation Phantom - PET Faceplate");
pub const ACRAccreditationPhantomStereotacticBreastBiopsy: Code<'static> = c("113685", "ACR Accreditation Phantom - Stereotactic Breast Biopsy");
pub const ACRGuidelinePerformanceOfAdultCTColonography: Code<'static> = c("112248", "ACR Guideline, Performance of Adult CT Colonography");
pub const ACRPositionStatement: Code<'static> = c("112036", "ACR Position Statement");
pub const ACRStandardCTMedicalPhysicsPerformanceMonitoring: Code<'static> = c("112249", "ACR Standard, CT medical physics performance monitoring");
pub const ADCHyperintense: Code<'static> = c("130595", "ADC Hyperintense");
pub const ADCHypointense: Code<'static> = c("130596", "ADC Hypointense");
pub const AF10: Code<'static> = c("130713", "AF10");
pub const AF9: Code<'static> = c("130712", "AF9");
pub const AGN15099889Zr: Code<'static> = c("126729", "AGN-150998 ^89^Zr");
pub const AIBasedMAR: Code<'static> = c("131544", "AI-Based MAR");
pub const AIC: Code<'static> = c("126223", "AIC");
pub const AIFIgnored: Code<'static> = c("126360", "AIF Ignored");
pub const AIN76: Code<'static> = c("127272", "AIN76");
pub const AIN93G: Code<'static> = c("127273", "AIN93G");
pub const AIN93M: Code<'static> = c("127274", "AIN93M");
pub const ANGPattern: Code<'static> = c("109902", "ANG Pattern");
pub const AP45: Code<'static> = c("112301", "AP-45");
pub const APPlus45: Code<'static> = c("112300", "AP+45");
pub const ARWave: Code<'static> = c("125323", "AR-wave");
pub const ATSMCu60: Code<'static> = c("126700", "ATSM Cu^60^");
pub const ATSMCu61: Code<'static> = c("126701", "ATSM Cu^61^");
pub const ATSMCu62: Code<'static> = c("126702", "ATSM Cu^62^");
pub const AWavePeakPressure: Code<'static> = c("109016", "A wave peak pressure");
pub const AWavePressureAverage: Code<'static> = c("109017", "A wave pressure, average");
pub const AbdominalAttachmentOfUmbilicalCord: Code<'static> = c("131371", "Abdominal attachment of umbilical cord");
pub const AbdominalCompressionArch: Code<'static> = c("130655", "Abdominal Compression Arch");
pub const AbdominalCompressionBelt: Code<'static> = c("130654", "Abdominal Compression Belt");
pub const AbdominalCompressionBeltLength: Code<'static> = c("130660", "Abdominal Compression Belt Length");
pub const AbdominalCompressionBeltPressure: Code<'static> = c("130661", "Abdominal Compression Belt Pressure");
pub const AbdominalCompressionPlatePositionNumber: Code<'static> = c("130659", "Abdominal Compression Plate Position Number");
pub const AbdominalImagingSpecialty: Code<'static> = c("128006", "Abdominal Imaging Specialty");
pub const AbdominalRadiology: Code<'static> = c("128170", "Abdominal Radiology");
pub const AbdominalRespiratoryEffort: Code<'static> = c("130432", "Abdominal Respiratory Effort");
pub const AbdominalRespiratoryInductance: Code<'static> = c("130428", "Abdominal Respiratory Inductance");
pub const AbdominalRespiratoryPVDF: Code<'static> = c("130430", "Abdominal Respiratory PVDF");
pub const AblationOff: Code<'static> = c("109038", "Ablation off");
pub const AblationOn: Code<'static> = c("109039", "Ablation on");
pub const AbnormalCalcifications: Code<'static> = c("112063", "Abnormal calcifications");
pub const AbnormalDischarge: Code<'static> = c("111491", "Abnormal discharge");
pub const AbnormalDistributionOfAnatomicStructure: Code<'static> = c("112028", "Abnormal Distribution of Anatomic Structure");
pub const AbnormalInterstitialPattern: Code<'static> = c("112004", "Abnormal interstitial pattern");
pub const AbnormalLines1D: Code<'static> = c("112061", "Abnormal lines (1D)");
pub const AbnormalLucency: Code<'static> = c("112062", "Abnormal lucency");
pub const AbnormalOpacity: Code<'static> = c("112033", "Abnormal opacity");
pub const AbnormalRegion: Code<'static> = c("122451", "Abnormal Region");
pub const AbnormalTexture: Code<'static> = c("112064", "Abnormal texture");
pub const AbnormallyHighSensitivity: Code<'static> = c("111849", "Abnormally high sensitivity");
pub const AbruptDuctTermination: Code<'static> = c("111427", "Abrupt duct termination");
pub const AbruptInterface: Code<'static> = c("111358", "Abrupt interface");
#[deprecated = "retired in PS3.16"]
pub const Absent: Code<'static> = c("121054", "Absent");
pub const AbsoluteOphthalmicThickness: Code<'static> = c("111930", "Absolute ophthalmic thickness");
pub const AbsoluteRegionalBloodFlow: Code<'static> = c("126390", "Absolute Regional Blood Flow");
pub const AbsoluteRegionalBloodVolume: Code<'static> = c("126391", "Absolute Regional Blood Volume");
pub const AbsoluteValuesOfVentricularMeasurements: Code<'static> = c("122608", "Absolute Values Of Ventricular Measurements");
pub const AbsorbedDose: Code<'static> = c("128513", "Absorbed Dose");
pub const AcceptableUncertaintyInDoseResult: Code<'static> = c("131326", "Acceptable Uncertainty in Dose Result");
pub const AccessControlDecision: Code<'static> = c("110147", "Access Control Decision");
pub const AccessionNumber: Code<'static> = c("121022", "Accession Number");
pub const AccessoryClearanceInterlock: Code<'static> = c("130742", "Accessory Clearance Interlock");
pub const AccessoryTray: Code<'static> = c("130124", "Accessory Tray");
pub const AccumulatedAverageGlandularDose: Code<'static> = c("111637", "Accumulated Average Glandular Dose");
pub const AccumulatedCTDIvolForwardEstimate: Code<'static> = c("113906", "Accumulated CTDIvol Forward Estimate");
pub const AccumulatedDLPForwardEstimate: Code<'static> = c("113905", "Accumulated DLP Forward Estimate");
pub const AccumulatedDoseData: Code<'static> = c("130500", "Accumulated Dose Data");
pub const AccumulatedXRayDoseData: Code<'static> = c("113702", "Accumulated X-Ray Dose Data");
pub const AcetabularAngle: Code<'static> = c("121224", "Acetabular angle");
pub const AcetabularCenterOfRotation: Code<'static> = c("112303", "Acetabular Center of Rotation");
pub const AcetabularCupInsert: Code<'static> = c("112306", "Acetabular Cup Insert");
pub const AcetabularCupMonoblock: Code<'static> = c("112307", "Acetabular Cup Monoblock");
pub const AcetabularCupShell: Code<'static> = c("112305", "Acetabular Cup Shell");
pub const Acinar: Code<'static> = c("112146", "Acinar");
pub const AcquiredImage: Code<'static> = c("113795", "Acquired Image");
pub const AcquiredVolume: Code<'static> = c("128160", "Acquired Volume");
pub const AcquisitionDate: Code<'static> = c("126201", "Acquisition Date");
pub const AcquisitionDateTime: Code<'static> = c("130884", "Acquisition DateTime");
pub const AcquisitionDeviceID: Code<'static> = c("122143", "Acquisition Device ID");
pub const AcquisitionDeviceType: Code<'static> = c("122142", "Acquisition Device Type");
pub const AcquisitionDoseAreaProductTotal: Code<'static> = c("113727", "Acquisition Dose Area Product Total");
pub const AcquisitionDoseRPTotal: Code<'static> = c("113729", "Acquisition Dose (RP) Total");
pub const AcquisitionDuration: Code<'static> = c("121142", "Acquisition Duration");
pub const AcquisitionEquipment: Code<'static> = c("109101", "Acquisition Equipment");
pub const AcquisitionEquipmentAlignment: Code<'static> = c("125023", "Acquisition Equipment Alignment");
pub const AcquisitionFramesCorrespondingToVolume: Code<'static> = c("121346", "Acquisition frames corresponding to volume");
pub const AcquisitionInitiationAfterEndOfRadiation: Code<'static> = c("130794", "Acquisition Initiation after end of Radiation");
pub const AcquisitionInitiationBeforeStartOfRadiation: Code<'static> = c("130793", "Acquisition Initiation before start of Radiation");
pub const AcquisitionInitiationByTriggeringParameter: Code<'static> = c("130795", "Acquisition Initiation by triggering parameter");
pub const AcquisitionInitiationNotDefined: Code<'static> = c("130792", "Acquisition Initiation not defined");
pub const AcquisitionInitiationType: Code<'static> = c("130791", "Acquisition Initiation Type");
pub const AcquisitionNumber: Code<'static> = c("113608", "Acquisition Number");
pub const AcquisitionPlane: Code<'static> = c("113764", "Acquisition Plane");
pub const AcquisitionProtocol: Code<'static> = c("125203", "Acquisition Protocol");
pub const AcquisitionTime: Code<'static> = c("126202", "Acquisition Time");
pub const ActionOnFluid: Code<'static> = c("111456", "Action on fluid");
pub const ActiveIngredientUndilutedConcentration: Code<'static> = c("121380", "Active Ingredient Undiluted Concentration");
pub const ActiveSurveillanceOfProstateCancer: Code<'static> = c("130589", "Active surveillance of prostate cancer");
pub const ActivityMeasurementDevice: Code<'static> = c("113540", "Activity Measurement Device");
pub const ActivityWorkload: Code<'static> = c("122709", "Activity workload");
pub const AcuteMIPresent: Code<'static> = c("122172", "Acute MI Present");
pub const AcuteUnilateralRenalBlockage: Code<'static> = c("113560", "Acute unilateral renal blockage");
pub const AdaptiveFiltering: Code<'static> = c("123102", "Adaptive Filtering");
pub const AdaptiveRadiationTherapyNote: Code<'static> = c("130039", "Adaptive Radiation Therapy Note");
pub const AddAddendumToReport: Code<'static> = c("128001", "Add Addendum to Report");
pub const AddIntermediate: Code<'static> = c("111673", "Add Intermediate");
pub const AddNear: Code<'static> = c("111672", "Add Near");
pub const AddOther: Code<'static> = c("111674", "Add Other");
pub const AddendedReport: Code<'static> = c("121361", "Addended report");
#[deprecated = "retired in PS3.16"]
pub const Addendum: Code<'static> = c("121078", "Addendum");
pub const AdditionalEvaluationRequestedFromAbnormalScreeningStudy: Code<'static> = c("111419", "Additional evaluation requested from abnormal screening study");
pub const AdditionalEvaluationRequestedFromPriorStudy: Code<'static> = c("111415", "Additional evaluation requested from prior study");
pub const AdditionalProjections: Code<'static> = c("111135", "Additional projections");
pub const AdditionalTeachingFileInformation: Code<'static> = c("128712", "Additional Teaching File Information");
#[deprecated = "retired in PS3.16"]
pub const Adenolipoma: Code<'static> = c("111248", "Adenolipoma");
#[deprecated = "retired in PS3.16"]
pub const Adenomyoepithelioma: Code<'static> = c("111250", "Adenomyoepithelioma");
pub const AdequatelyStained: Code<'static> = c("131507", "Adequately stained");
pub const AdhocMeasurements: Code<'static> = c("125303", "Adhoc Measurements");
pub const AdiabaticTissueHomogeneityModel: Code<'static> = c("126346", "Adiabatic Tissue Homogeneity Model");
pub const AdministeredActivity: Code<'static> = c("113507", "Administered activity");
pub const AdministrationDiscontinued: Code<'static> = c("130220", "Administration discontinued");
pub const AdministrationMode: Code<'static> = c("130181", "Administration Mode");
pub const AdministrationStepType: Code<'static> = c("130250", "Administration Step Type");
pub const AdministratorOfRadiologyDepartment: Code<'static> = c("128673", "Administrator of Radiology Department");
pub const AdultCongenitalCardiacUltrasoundReport: Code<'static> = c("125197", "Adult Congenital Cardiac Ultrasound Report");
pub const AdultEchocardiographyProcedureReport: Code<'static> = c("125200", "Adult Echocardiography Procedure Report");
pub const AdvancedLevel: Code<'static> = c("128727", "Advanced level");
pub const AdventitialDissection: Code<'static> = c("122397", "Adventitial Dissection");
pub const AdverseEventDetectionDateTime: Code<'static> = c("130215", "Adverse Event Detection DateTime");
pub const AerobicIndex: Code<'static> = c("122772", "Aerobic index");
pub const AgatstonScoreThreshold: Code<'static> = c("122657", "Agatston Score Threshold");
pub const AgatstonScoringMethod: Code<'static> = c("112055", "Agatston scoring method");
pub const AgeAtFirstFullTermPregnancy: Code<'static> = c("111519", "Age at First Full Term Pregnancy");
pub const AgeAtMenopause: Code<'static> = c("111520", "Age at Menopause");
pub const AgeAtOccurrence: Code<'static> = c("111538", "Age at Occurrence");
pub const AgeCorrected: Code<'static> = c("111838", "Age corrected");
pub const AgeEnded: Code<'static> = c("111525", "Age Ended");
pub const AgeStarted: Code<'static> = c("111524", "Age Started");
pub const AgeWhenFirstMenstrualPeriodOccurred: Code<'static> = c("111518", "Age when first menstrual period occurred");
pub const AgeWhenHysterectomyPerformed: Code<'static> = c("111521", "Age when hysterectomy performed");
pub const AgeWhenLeftOvaryRemoved: Code<'static> = c("111522", "Age when left ovary removed");
pub const AgeWhenRightOvaryRemoved: Code<'static> = c("111523", "Age when right ovary removed");
pub const AgreementWithPriorFindings: Code<'static> = c("122775", "Agreement with prior findings");
pub const Air: Code<'static> = c("112143", "Air");
pub const AirBronchiologram: Code<'static> = c("112070", "Air bronchiologram");
pub const AirBronchogram: Code<'static> = c("112071", "Air bronchogram");
pub const AirChanges: Code<'static> = c("127172", "Air changes");
pub const AirCrescent: Code<'static> = c("112072", "Air crescent");
pub const AirDetected: Code<'static> = c("130175", "Air detected");
pub const AirFluidLevel: Code<'static> = c("112104", "Air-fluid level");
pub const AirKermaAtOutputMeasurementPoint: Code<'static> = c("130515", "Air Kerma at Output Measurement Point");
pub const AirSpace: Code<'static> = c("112147", "Air space");
pub const Airflow: Code<'static> = c("130421", "Airflow");
pub const AirflowCPAP: Code<'static> = c("130420", "Airflow CPAP");
pub const AirflowNasalProng: Code<'static> = c("130418", "Airflow Nasal Prong");
pub const AirflowPVDF: Code<'static> = c("130419", "Airflow PVDF");
pub const AirflowThermistor: Code<'static> = c("130416", "Airflow Thermistor");
pub const AirflowThermocouple: Code<'static> = c("130417", "Airflow Thermocouple");
pub const AirwayManagementMethod: Code<'static> = c("127312", "Airway Management Method");
pub const AirwayManagementSet: Code<'static> = c("127310", "Airway Management Set");
pub const AirwayPartiallyObstructed: Code<'static> = c("122282", "Airway partially obstructed");
pub const AirwaySeverelyObstructed: Code<'static> = c("122283", "Airway severely obstructed");
pub const AirwaySubManagementMethod: Code<'static> = c("127313", "Airway Sub-Management Method");
pub const AirwayUnobstructed: Code<'static> = c("122281", "Airway unobstructed");
pub const AlgorithmFamily: Code<'static> = c("111000", "Algorithm Family");
pub const AlgorithmManufacturer: Code<'static> = c("122405", "Algorithm Manufacturer");
pub const AlgorithmName: Code<'static> = c("111001", "Algorithm Name");
pub const AlgorithmParameters: Code<'static> = c("111002", "Algorithm Parameters");
pub const AlgorithmVersion: Code<'static> = c("111003", "Algorithm Version");
pub const AlgorithmicAttenuationCorrection: Code<'static> = c("122726", "Algorithmic attenuation correction");
pub const AllAlgorithmsSucceededWithFindings: Code<'static> = c("111242", "All algorithms succeeded; with findings");
pub const AllAlgorithmsSucceededWithoutFindings: Code<'static> = c("111241", "All algorithms succeeded; without findings");
pub const AllPlanes: Code<'static> = c("113890", "All Planes");
pub const AlphaGEUDValue: Code<'static> = c("130095", "alpha gEUD Value");
pub const AlternatePhotometricInterpretationImage: Code<'static> = c("121328", "Alternate Photometric Interpretation image");
pub const AlternateSOPClassInstance: Code<'static> = c("121326", "Alternate SOP Class instance");
pub const AlternativeDoseAlertBehaviorActive: Code<'static> = c("113915", "Alternative dose alert behavior active");
pub const AmbientRoomEnvironment: Code<'static> = c("109112", "Ambient room environment");
pub const AmbiguousSex: Code<'static> = c("121104", "Ambiguous sex");
pub const AmericaKennelClub: Code<'static> = c("109200", "America Kennel Club");
pub const AmericanCanineAssociation: Code<'static> = c("109202", "American Canine Association");
pub const AmericanPurebredRegistry: Code<'static> = c("109203", "American Purebred Registry");
pub const AmericanRareBreedAssociation: Code<'static> = c("109204", "American Rare Breed Association");
pub const AmericasPetRegistryInc: Code<'static> = c("109201", "America's Pet Registry Inc.");
pub const AnalysisPerformed: Code<'static> = c("111004", "Analysis Performed");
pub const AnalysisQualityRating: Code<'static> = c("111693", "Analysis Quality Rating");
pub const AnalyticalAlgorithm: Code<'static> = c("128480", "Analytical Algorithm");
pub const AnatomicIdentifier: Code<'static> = c("112050", "Anatomic Identifier");
pub const AnatomicImage: Code<'static> = c("121338", "Anatomic image");
pub const AnatomicNonColon: Code<'static> = c("112238", "Anatomic non-colon");
pub const AnatomicalAxisOfFemur: Code<'static> = c("112302", "Anatomical axis of femur");
pub const AnatomyCoverageIncomplete: Code<'static> = c("130576", "Anatomy coverage incomplete");
pub const Anechoic: Code<'static> = c("111361", "Anechoic");
pub const AnesthesiaCategory: Code<'static> = c("127302", "Anesthesia Category");
pub const AnesthesiaMethod: Code<'static> = c("127301", "Anesthesia Method");
pub const AnesthesiaMethodSet: Code<'static> = c("127300", "Anesthesia Method Set");
pub const AnesthesiaSubCategory: Code<'static> = c("127303", "Anesthesia SubCategory");
pub const AneurysmOnCitedVessel: Code<'static> = c("122101", "Aneurysm on cited vessel");
pub const AnginalEquivalent: Code<'static> = c("122799", "Anginal Equivalent");
pub const AngioplastyBalloonDeflated: Code<'static> = c("122304", "Angioplasty balloon deflated");
pub const AngioplastyBalloonInflated: Code<'static> = c("122303", "Angioplasty balloon inflated");
#[deprecated = "retired in PS3.16"]
pub const Angiosarcoma: Code<'static> = c("111302", "Angiosarcoma");
#[deprecated = "retired in PS3.16"]
pub const Angioscopy: Code<'static> = c("AS", "Angioscopy");
pub const Angle: Code<'static> = c("110859", "Angle");
pub const AngularMargins: Code<'static> = c("111343", "Angular margins");
pub const AngularModulation: Code<'static> = c("114101", "Angular modulation");
#[deprecated = "retired in PS3.16"]
pub const AngularSecondMomentOfGLCM: Code<'static> = c("126065", "Angular Second Moment of GLCM");
pub const AngularViewingEvaluation: Code<'static> = c("109709", "Angular viewing evaluation");
pub const AnimalHandlingDuringSpecifiedPhase: Code<'static> = c("127005", "Animal handling during specified phase");
pub const AnimalHousing: Code<'static> = c("127120", "Animal housing");
pub const AnimalHousingRoom: Code<'static> = c("127370", "Animal housing room");
pub const AnimalRegistryUnlimited: Code<'static> = c("109205", "Animal Registry Unlimited");
pub const AnimalResearchFoundation: Code<'static> = c("109206", "Animal Research Foundation");
pub const AnimalRoomIdentifier: Code<'static> = c("127122", "Animal room identifier");
pub const AnimalRoomType: Code<'static> = c("127121", "Animal room type");
pub const Annotation: Code<'static> = c("ANN", "Annotation");
pub const AnnotationNote: Code<'static> = c("130876", "Annotation Note");
pub const AnodeTargetMaterial: Code<'static> = c("111632", "Anode Target Material");
pub const AnomalousExponentParameter: Code<'static> = c("113299", "Anomalous Exponent Parameter");
pub const AntegradeDuctusVenosus: Code<'static> = c("131379", "Antegrade ductus venosus");
pub const AnteriorFirst: Code<'static> = c("126833", "anterior first");
pub const AnteriorInferiorAxis: Code<'static> = c("110864", "Anterior-Inferior Axis");
pub const AnteriorJunctionLine: Code<'static> = c("112088", "Anterior junction line");
pub const AnteriorPosterior: Code<'static> = c("122675", "Anterior-Posterior");
pub const AnteriorPosteriorAxis: Code<'static> = c("110862", "Anterior-Posterior Axis");
pub const AnteriorToInferior: Code<'static> = c("110874", "Anterior To Inferior");
pub const AnteriorToPosterior: Code<'static> = c("110870", "Anterior To Posterior");
pub const AnterolateralPapillaryMuscleToTheLeftTrigone: Code<'static> = c("131192", "Anterolateral papillary muscle to the left trigone");
pub const AnthropomorphicModel: Code<'static> = c("128404", "Anthropomorphic Model");
pub const AntiB22089Zr: Code<'static> = c("126754", "Anti-B220 ^89^Zr");
pub const AntralFollicle: Code<'static> = c("130910", "Antral Follicle");
pub const AnyDecisionToBiopsyShouldBeBasedOnClinicalAssessment: Code<'static> = c("111141", "Any decision to biopsy should be based on clinical assessment");
pub const AortaSinotubularJunctionArea: Code<'static> = c("131140", "Aorta sinotubular junction area");
pub const AorticAnnulusCalcificationSeverity: Code<'static> = c("131142", "Aortic annulus calcification severity");
pub const AorticAnnulusMaxDiameter: Code<'static> = c("131143", "Aortic annulus max diameter");
pub const AorticAnnulusMinDiameter: Code<'static> = c("131144", "Aortic annulus min diameter");
pub const AorticAnnulusPerimeter: Code<'static> = c("131145", "Aortic annulus perimeter");
pub const AorticArchView: Code<'static> = c("131022", "Aortic arch view");
pub const AorticCalcificationVolume: Code<'static> = c("131146", "Aortic calcification volume");
pub const AorticCommissuresCalcificationSeverity: Code<'static> = c("131147", "Aortic commissures calcification severity");
pub const AorticIntervention: Code<'static> = c("122054", "Aortic Intervention");
pub const AorticKnob: Code<'static> = c("112102", "Aortic knob");
pub const AorticRootHeight: Code<'static> = c("131148", "Aortic root height");
pub const AorticSinotubularJunctionDiameter: Code<'static> = c("131149", "Aortic sinotubular junction diameter");
pub const AorticSinusOfValsalvaArea: Code<'static> = c("131150", "Aortic sinus of valsalva area");
pub const AorticValveClosure: Code<'static> = c("130701", "Aortic Valve Closure");
pub const AorticValveCoaptationLength: Code<'static> = c("131152", "Aortic valve coaptation length");
pub const AorticValveLeftCoronaryLeafletHeight: Code<'static> = c("131159", "Aortic valve left coronary leaflet height");
pub const AorticValveLeftCoronaryLeafletLength: Code<'static> = c("131160", "Aortic valve left coronary leaflet length");
pub const AorticValveLeftLeafletIntercommissuralAngle: Code<'static> = c("131155", "Aortic valve left leaflet intercommissural angle");
pub const AorticValveLeftLeafletIntercommissuralDistance: Code<'static> = c("131158", "Aortic valve left leaflet intercommissural distance");
pub const AorticValveNoncoronaryLeafletHeight: Code<'static> = c("131161", "Aortic valve noncoronary leaflet height");
pub const AorticValveNoncoronaryLeafletIntercommissuralAngle: Code<'static> = c("131153", "Aortic valve noncoronary leaflet intercommissural angle");
pub const AorticValveNoncoronaryLeafletIntercommissuralDistance: Code<'static> = c("131156", "Aortic valve noncoronary leaflet intercommissural distance");
pub const AorticValveNoncoronaryLeafletLength: Code<'static> = c("131162", "Aortic valve noncoronary leaflet length");
pub const AorticValveOpening: Code<'static> = c("130702", "Aortic Valve Opening");
pub const AorticValveRightCoronaryLeafletHeight: Code<'static> = c("131163", "Aortic valve right coronary leaflet height");
pub const AorticValveRightCoronaryLeafletLength: Code<'static> = c("131164", "Aortic valve right coronary leaflet length");
pub const AorticValveRightLeafletIntercommissuralAngle: Code<'static> = c("131154", "Aortic valve right leaflet intercommissural angle");
pub const AorticValveRightLeafletIntercommissuralDistance: Code<'static> = c("131157", "Aortic valve right leaflet intercommissural distance");
pub const AortoMitralInterAnnularAngle: Code<'static> = c("131177", "Aorto-mitral inter annular angle");
pub const ApertureBlock: Code<'static> = c("130123", "Aperture Block");
pub const ApexBaseAxis: Code<'static> = c("110863", "Apex-Base Axis");
pub const ApexToBase: Code<'static> = c("110872", "Apex To Base");
pub const ApicalFourChamberRVFocused: Code<'static> = c("130681", "Apical four chamber RV Focused");
pub const ApicalFourChamberRVModified: Code<'static> = c("130682", "Apical four chamber RV Modified");
pub const ApparentDiffusionCoefficient: Code<'static> = c("113041", "Apparent Diffusion Coefficient");
pub const ApparentKurtosisCoefficient: Code<'static> = c("113206", "Apparent Kurtosis Coefficient");
pub const Application: Code<'static> = c("110150", "Application");
pub const ApplicationActivity: Code<'static> = c("110100", "Application Activity");
pub const ApplicationLauncher: Code<'static> = c("110151", "Application Launcher");
pub const ApplicationStart: Code<'static> = c("110120", "Application Start");
pub const ApplicationStop: Code<'static> = c("110121", "Application Stop");
pub const ApplicatorPresent: Code<'static> = c("130459", "Applicator Present");
pub const AppropriateForTheDevice: Code<'static> = c("128606", "Appropriate for the device");
pub const AppropriateForTheIndications: Code<'static> = c("128601", "Appropriate for the indications");
pub const ApprovedForExperimentalUse: Code<'static> = c("128611", "Approved for experimental use");
pub const ApprovedForUseAtTheInstitution: Code<'static> = c("128603", "Approved for use at the institution");
pub const ApprovedForUseInTheClinicalTrial: Code<'static> = c("128604", "Approved for use in the clinical trial");
pub const ApprovedForUseOnPregnantPatients: Code<'static> = c("128605", "Approved for use on pregnant patients");
pub const ApproximateSpatialLocation: Code<'static> = c("121226", "Approximate spatial location");
pub const ArcBeam: Code<'static> = c("130103", "Arc Beam");
pub const ArcOfCalcium: Code<'static> = c("122355", "Arc of Calcium");
pub const ArchOfTheAzygosVein: Code<'static> = c("112103", "Arch of the Azygos vein");
pub const ArchitecturalDistortion: Code<'static> = c("112079", "Architectural distortion");
pub const Archive: Code<'static> = c("ARCHIVE", "Archive");
pub const AreaBasedMethod: Code<'static> = c("122650", "Area Based Method");
pub const AreaByPressureHalfTime: Code<'static> = c("125210", "Area by Pressure Half-Time");
pub const AreaEqualsFlowPer445SqrtGradientMmHg: Code<'static> = c("122262", "Area = Flow / 44.5 * sqrt(Gradient[mmHg])");
pub const AreaLengthBiplane: Code<'static> = c("125204", "Area-length biplane");
pub const AreaLengthDodge: Code<'static> = c("122559", "Area Length Dodge");
pub const AreaLengthKennedy: Code<'static> = c("122558", "Area Length Kennedy");
pub const AreaLengthMethod: Code<'static> = c("122428", "Area Length Method");
pub const AreaLengthSinglePlane: Code<'static> = c("125205", "Area-Length Single Plane");
pub const AreaLengthWynne: Code<'static> = c("122560", "Area Length Wynne");
pub const AreaOfAClosedNURBS: Code<'static> = c("122502", "Area of a closed NURBS");
pub const AreaOfClosedIrregularPolygon: Code<'static> = c("122501", "Area of closed irregular polygon");
#[deprecated = "retired in PS3.16"]
pub const AreaOfDefinedRegion: Code<'static> = c("121202", "Area of Defined Region");
pub const AreaOfVascularCalcification: Code<'static> = c("112196", "Area of Vascular Calcification");
pub const AreaOutline: Code<'static> = c("121056", "Area outline");
#[deprecated = "retired in PS3.16"]
pub const AreaOutline_121201: Code<'static> = c("121201", "Area Outline");
pub const AreaSymmetry: Code<'static> = c("122545", "Area Symmetry");
pub const ArithmeticAverageOfDwZ: Code<'static> = c("113990", "Arithmetic Average of Dw(z)");
pub const ArithmeticAverageOfSSDEZ: Code<'static> = c("113989", "Arithmetic Average of SSDE(z)");
pub const ArmOfAngle: Code<'static> = c("121223", "Arm of angle");
pub const ArterialConduitAngiography: Code<'static> = c("122058", "Arterial conduit angiography");
pub const ArterialDopplerScore: Code<'static> = c("131035", "Arterial Doppler Score");
pub const ArterialPulseWaveform: Code<'static> = c("109116", "Arterial Pulse Waveform");
pub const ArteriovenousDifference: Code<'static> = c("122229", "Arteriovenous difference");
pub const ArtifactSOtherThanGridOrDetectorArtifact: Code<'static> = c("111215", "Artifact(s) other than grid or detector artifact");
pub const ArtificialIntelligence: Code<'static> = c("123110", "Artificial Intelligence");
pub const AscendingAortaDiameter: Code<'static> = c("131165", "Ascending Aorta diameter");
pub const AspenChipBedding: Code<'static> = c("127230", "Aspen chip bedding");
pub const AspenShavingBedding: Code<'static> = c("127231", "Aspen shaving bedding");
pub const Assembly: Code<'static> = c("112355", "Assembly");
pub const AssessChange: Code<'static> = c("130590", "Assess change");
pub const AssessmentByComparison: Code<'static> = c("121375", "Assessment By Comparison");
pub const AssessmentByRules: Code<'static> = c("121376", "Assessment By Rules");
pub const AssessmentCategory: Code<'static> = c("111005", "Assessment Category");
pub const Assisting: Code<'static> = c("121099", "Assisting");
pub const AssociatedChestComponent: Code<'static> = c("112003", "Associated Chest Component");
pub const AsymmetricImplants: Code<'static> = c("111504", "Asymmetric implants");
#[deprecated = "retired in PS3.16"]
pub const AsynchronousInvolution: Code<'static> = c("111254", "Asynchronous involution");
pub const AtLastAppointment: Code<'static> = c("109125", "At last appointment");
pub const AtSurfaceOfPatient: Code<'static> = c("113964", "At Surface of Patient");
pub const AtomicNumber: Code<'static> = c("130093", "Atomic Number");
pub const AtrialDiastolicFillingDWave: Code<'static> = c("125322", "Atrial Diastolic Filling (D-wave)");
pub const AtrialPressureMeasurements: Code<'static> = c("122121", "Atrial pressure measurements");
pub const AtrioventricularTimeInterval: Code<'static> = c("131002", "Atrioventricular time interval");
pub const Attach: Code<'static> = c("110124", "Attach");
#[deprecated = "retired in PS3.16"]
pub const AttendingConsultant: Code<'static> = c("121089", "Attending [Consultant]");
pub const AttenuationCoefficient: Code<'static> = c("112031", "Attenuation Coefficient");
pub const AttenuationCorrection: Code<'static> = c("122713", "Attenuation correction");
pub const AttenuatorCategory: Code<'static> = c("128458", "Attenuator Category");
pub const AttenuatorCharacteristics: Code<'static> = c("130531", "Attenuator Characteristics");
pub const AttenuatorDescription: Code<'static> = c("128468", "Attenuator Description");
pub const AttenuatorPosition: Code<'static> = c("130528", "Attenuator Position");
pub const Attenuators: Code<'static> = c("130513", "Attenuators");
pub const Audio: Code<'static> = c("AU", "Audio");
pub const AuditLogUsed: Code<'static> = c("110101", "Audit Log Used");
pub const AuditRecordingStarted: Code<'static> = c("110134", "Audit Recording Started");
pub const AuditRecordingStopped: Code<'static> = c("110133", "Audit Recording Stopped");
pub const AuthenticationDecision: Code<'static> = c("110143", "Authentication Decision");
pub const AuthorizationDecision: Code<'static> = c("110144", "Authorization Decision");
pub const AutoKeratometry: Code<'static> = c("111754", "Auto Keratometry");
#[deprecated = "retired in PS3.16"]
pub const AutocorrelationOfGLCM: Code<'static> = c("128794", "Autocorrelation of GLCM");
pub const AutomatedAdministration: Code<'static> = c("130173", "Automated Administration");
pub const AutomatedDataCollection: Code<'static> = c("113856", "Automated Data Collection");
pub const AutomatedManualInjectPhase: Code<'static> = c("130171", "Automated Manual Inject Phase");
pub const AutomatedOptical: Code<'static> = c("111843", "Automated Optical");
pub const AutomatedSlideStainer: Code<'static> = c("STAIN", "Automated Slide Stainer");
pub const AutomaticProgrammedAdministrationPhase: Code<'static> = c("130168", "Automatic Programmed Administration Phase");
pub const AutomaticProgrammedDelayPhase: Code<'static> = c("130169", "Automatic Programmed Delay Phase");
pub const AutomaticProgrammedWaitPhase: Code<'static> = c("130263", "Automatic Programmed Wait Phase");
#[deprecated = "retired in PS3.16"]
pub const AutomaticWithManualHoldPhase: Code<'static> = c("130170", "Automatic with Manual Hold Phase");
pub const AutomaticallyDetectedAIFROI: Code<'static> = c("126363", "Automatically Detected AIF ROI");
pub const Autorefraction: Code<'static> = c("AR", "Autorefraction");
pub const AutorefractionMeasurementsSOPInstance: Code<'static> = c("111784", "Autorefraction Measurements SOP Instance");
pub const AutorefractionVisualAcuity: Code<'static> = c("111685", "Autorefraction Visual Acuity");
pub const AvascularComplexFlow: Code<'static> = c("128306", "Avascular complex flow");
pub const AvascularComplexMap: Code<'static> = c("128307", "Avascular complex map");
pub const AverageBreastFeedingPeriod: Code<'static> = c("111544", "Average breast feeding period");
pub const AverageCalcificationDistanceInACalcificationCluster: Code<'static> = c("112200", "Average calcification distance in a calcification cluster");
pub const AverageGanglionCellThickness: Code<'static> = c("131288", "Average ganglion cell thickness");
pub const AverageGanglionCellThicknessInPosteriorPoleGrid: Code<'static> = c("131298", "Average ganglion cell thickness in posterior pole grid");
pub const AverageGanglionCellThicknessInferiorSector: Code<'static> = c("131294", "Average ganglion cell thickness inferior sector");
pub const AverageGanglionCellThicknessNasalInferiorSector: Code<'static> = c("131293", "Average ganglion cell thickness nasal-inferior sector");
pub const AverageGanglionCellThicknessNasalSector: Code<'static> = c("131292", "Average ganglion cell thickness nasal sector");
pub const AverageGanglionCellThicknessNasalSuperiorSector: Code<'static> = c("131291", "Average ganglion cell thickness nasal-superior sector");
pub const AverageGanglionCellThicknessSuperiorSector: Code<'static> = c("131290", "Average ganglion cell thickness superior sector");
pub const AverageGanglionCellThicknessTemporalInferiorSector: Code<'static> = c("131295", "Average ganglion cell thickness temporal-inferior sector");
pub const AverageGanglionCellThicknessTemporalSector: Code<'static> = c("131296", "Average ganglion cell thickness temporal sector");
pub const AverageGanglionCellThicknessTemporalSuperiorSector: Code<'static> = c("131297", "Average ganglion cell thickness temporal-superior sector");
pub const AverageGlandularDose: Code<'static> = c("111631", "Average Glandular Dose");
pub const AverageMacularThickness: Code<'static> = c("131255", "Average macular thickness");
pub const AverageXRayTubeCurrent: Code<'static> = c("113767", "Average X-Ray Tube Current");
pub const AvoidanceVolume: Code<'static> = c("130058", "Avoidance Volume");
pub const AxialDiffusivity: Code<'static> = c("113204", "Axial Diffusivity");
pub const AxialKurtosis: Code<'static> = c("113208", "Axial Kurtosis");
pub const AxialMeasurementsSOPInstance: Code<'static> = c("111782", "Axial Measurements SOP Instance");
pub const AxillaryNodalMetastases: Code<'static> = c("111301", "Axillary nodal metastases");
pub const AxillaryNodeHyperplasia: Code<'static> = c("111253", "Axillary node hyperplasia");
pub const AxillaryNodeWithCalcifications: Code<'static> = c("111252", "Axillary node with calcifications");
pub const AxillaryNodeWithLymphoma: Code<'static> = c("111300", "Axillary node with lymphoma");
pub const AzygoesophagealRecessInterface: Code<'static> = c("112090", "Azygoesophageal recess interface");
pub const BIC: Code<'static> = c("126224", "BIC");
pub const BMIEqualsWtPerHt2: Code<'static> = c("122265", "BMI = Wt/Ht^2");
pub const BN01Pattern: Code<'static> = c("109904", "BN01 Pattern");
pub const BN02Pattern: Code<'static> = c("109905", "BN02 Pattern");
pub const BN03Pattern: Code<'static> = c("109906", "BN03 Pattern");
pub const BN04Pattern: Code<'static> = c("109907", "BN04 Pattern");
pub const BN05Pattern: Code<'static> = c("109908", "BN05 Pattern");
pub const BN06Pattern: Code<'static> = c("109909", "BN06 Pattern");
pub const BN07Pattern: Code<'static> = c("109910", "BN07 Pattern");
pub const BN08Pattern: Code<'static> = c("109911", "BN08 Pattern");
pub const BN09Pattern: Code<'static> = c("109912", "BN09 Pattern");
pub const BN10Pattern: Code<'static> = c("109913", "BN10 Pattern");
pub const BN11Pattern: Code<'static> = c("109914", "BN11 Pattern");
pub const BN12Pattern: Code<'static> = c("109915", "BN12 Pattern");
pub const BN13Pattern: Code<'static> = c("109916", "BN13 Pattern");
pub const BN14Pattern: Code<'static> = c("109917", "BN14 Pattern");
pub const BN15Pattern: Code<'static> = c("109918", "BN15 Pattern");
pub const BN16Pattern: Code<'static> = c("109919", "BN16 Pattern");
pub const BN17Pattern: Code<'static> = c("109920", "BN17 Pattern");
pub const BN18Pattern: Code<'static> = c("109921", "BN18 Pattern");
pub const BRCA1BreastCancerGene: Code<'static> = c("111556", "BRCA1 breast cancer gene");
pub const BRCA2BreastCancerGene: Code<'static> = c("111557", "BRCA2 breast cancer gene");
pub const BRCA3BreastCancerGene: Code<'static> = c("111558", "BRCA3 breast cancer gene");
pub const BREPRadiationTransportModel: Code<'static> = c("128406", "BREP Radiation Transport Model");
pub const BSA1219: Code<'static> = c("122572", "BSA^1.219");
pub const BSAEquals00004688WT0816800154LogWT: Code<'static> = c("122246", "BSA = 0.0004688*WT^(0.8168-0.0154\u{200b}*log(WT))");
pub const BSAEquals0003207WT0728500188LogWTHT03: Code<'static> = c("122240", "BSA = 0.003207*WT^(0.7285-0.0188\u{200b}*log(WT))*HT^0.3");
pub const BSAEquals0007184WT0425HT0725: Code<'static> = c("122241", "BSA = 0.007184*WT^0.425\u{200b}*HT^0.725");
pub const BSAEquals0007358WT0425HT0725: Code<'static> = c("122266", "BSA = 0.007358*WT^0.425\u{200b}*HT^0.725");
pub const BSAEquals0008883WT0444HT0663: Code<'static> = c("122268", "BSA = 0.008883*WT^0.444\u{200b}*HT^0.663");
pub const BSAEquals0009568WT0473HT0655: Code<'static> = c("122270", "BSA = 0.009568*WT^0.473\u{200b}*HT^0.655");
pub const BSAEquals0010265WT0423HT0651: Code<'static> = c("122267", "BSA = 0.010265*WT^0.423\u{200b}*HT^0.651");
pub const BSAEquals00235WT051456HT042246: Code<'static> = c("122242", "BSA = 0.0235*WT^0.51456\u{200b}*HT^0.42246");
pub const BSAEquals0024265WT05378HT03964: Code<'static> = c("122243", "BSA = 0.024265*WT^0.5378\u{200b}*HT^0.3964");
pub const BSAEquals0038189WT0423HT0362: Code<'static> = c("122269", "BSA = 0.038189*WT^0.423\u{200b}*HT^0.362");
pub const BSAEquals1321Plus03433WT: Code<'static> = c("122245", "BSA = 1321+0.3433*WT");
pub const BSAEqualsHTWTPer3605: Code<'static> = c("122244", "BSA = (HT*WT/36)^0.5");
pub const BackOfCorneaToFrontOfLens: Code<'static> = c("111777", "Back Of Cornea To Front Of Lens");
pub const Background: Code<'static> = c("125040", "Background");
pub const BackgroundForLesionToBackgroundSUVRatio: Code<'static> = c("126040", "Background for Lesion to Background SUV Ratio");
pub const BackrestPitchAngle: Code<'static> = c("130842", "Backrest Pitch Angle");
pub const Backscatter: Code<'static> = c("128411", "Backscatter");
pub const BallAndDiskTypeLeftAtrialAppendageClosureDevice: Code<'static> = c("131110", "Ball and disk type left atrial appendage closure device");
pub const BallTypeLeftAtrialAppendageClosureDevice: Code<'static> = c("131111", "Ball type left atrial appendage closure device");
pub const BarcodeValue: Code<'static> = c("130231", "Barcode Value");
pub const BarrettDesignFactor: Code<'static> = c("111867", "Barrett Design Factor");
pub const BarrettKeratoconus: Code<'static> = c("111871", "Barrett Keratoconus");
pub const BarrettLensFactor: Code<'static> = c("111866", "Barrett Lens Factor");
pub const BarrettRx: Code<'static> = c("111872", "Barrett Rx");
pub const BarrettToric: Code<'static> = c("111862", "Barrett Toric");
pub const BarrettTrueK: Code<'static> = c("111863", "Barrett True-K");
pub const BarrettTrueKToric: Code<'static> = c("111864", "Barrett True-K Toric");
pub const BarrettUniversalII: Code<'static> = c("111865", "Barrett Universal II");
pub const BasalCellCarcinomaOfNipple: Code<'static> = c("111307", "Basal cell carcinoma of nipple");
pub const BaseToApex: Code<'static> = c("110873", "Base To Apex");
#[deprecated = "retired in PS3.16"]
pub const Baseline: Code<'static> = c("121079", "Baseline");
pub const BaselineCategory: Code<'static> = c("112016", "Baseline Category");
pub const BaselineResultCorrection: Code<'static> = c("122628", "Baseline Result Correction");
pub const BaselineScreeningMammogram: Code<'static> = c("111403", "Baseline screening mammogram");
#[deprecated = "retired in PS3.16"]
pub const BaselineStenosisMeasurement: Code<'static> = c("122107", "Baseline Stenosis Measurement");
pub const BaselineTIMIFlow: Code<'static> = c("122109", "Baseline TIMI Flow");
pub const BasicApplicationConfidentialityProfile: Code<'static> = c("113100", "Basic Application Confidentiality Profile");
pub const BasisImageDecomposition: Code<'static> = c("131567", "Basis image decomposition");
pub const BasisImageForSpectralProcessing: Code<'static> = c("131566", "Basis image for spectral processing");
pub const BatsWingDistribution: Code<'static> = c("112154", "Bat's wing distribution");
pub const BayesianProbability: Code<'static> = c("113270", "Bayesian-Probability");
pub const BazettQTcAlgorithm: Code<'static> = c("122730", "Bazett QTc Algorithm");
pub const BeadedSeptumSign: Code<'static> = c("112066", "Beaded septum sign");
pub const BeamBlock: Code<'static> = c("128431", "Beam Block");
pub const BeamEnergyInterlock: Code<'static> = c("130831", "Beam Energy Interlock");
pub const BeamPosition: Code<'static> = c("130524", "Beam Position");
pub const BeamShapingNote: Code<'static> = c("130023", "Beam Shaping Note");
pub const BeamTargetingToleranceViolation: Code<'static> = c("130468", "Beam Targeting Tolerance Violation");
pub const BeatDetectedAccepted: Code<'static> = c("109018", "Beat detected (accepted)");
pub const BeatDetectedRejected: Code<'static> = c("109019", "Beat detected (rejected)");
pub const BeatNumber: Code<'static> = c("122149", "Beat Number");
pub const BeddingDepth: Code<'static> = c("127185", "Bedding depth");
pub const BeddingManufacturer: Code<'static> = c("127180", "Bedding manufacturer");
pub const BeddingMass: Code<'static> = c("127184", "Bedding mass");
pub const BeddingProductCode: Code<'static> = c("127182", "Bedding product code");
pub const BeddingProductName: Code<'static> = c("127181", "Bedding product name");
pub const BeddingVolume: Code<'static> = c("127183", "Bedding volume");
pub const BeginCirculatorySupport: Code<'static> = c("121157", "Begin Circulatory Support");
pub const BeginOxygenAdministration: Code<'static> = c("121161", "Begin Oxygen Administration");
pub const BeginPacing: Code<'static> = c("121166", "Begin Pacing");
pub const BeginTransferringDICOMInstances: Code<'static> = c("110102", "Begin Transferring DICOM Instances");
pub const BeginVentilation: Code<'static> = c("121168", "Begin Ventilation");
pub const BeginningOfTimePeriod: Code<'static> = c("130533", "Beginning of Time Period");
pub const BehaviorOfTheFindingSite: Code<'static> = c("125312", "Behavior of the Finding Site");
pub const BenignCalcifications: Code<'static> = c("111256", "Benign Calcifications");
pub const BenignCystWithBlood: Code<'static> = c("111255", "Benign cyst with blood");
pub const Benralizumab89Zr: Code<'static> = c("126722", "Benralizumab ^89^Zr");
pub const BesselFilter: Code<'static> = c("130763", "Bessel filter");
pub const BestIllustrationOfFinding: Code<'static> = c("121080", "Best illustration of finding");
pub const BestInSet: Code<'static> = c("113013", "Best In Set");
pub const BestOverallResponse: Code<'static> = c("112049", "Best Overall Response");
pub const Bevacizumab89Zr: Code<'static> = c("126516", "Bevacizumab ^89^Zr");
pub const BiExponentialIVIMDiffusionModel: Code<'static> = c("113251", "Bi-exponential (IVIM) diffusion model");
pub const BiPlaneAngiography: Code<'static> = c("122060", "Bi-plane Angiography");
pub const BillingCode: Code<'static> = c("121147", "Billing Code");
pub const BinarySegmentation: Code<'static> = c("110853", "Binary Segmentation");
pub const BiomagneticImaging: Code<'static> = c("BI", "Biomagnetic imaging");
pub const BiomedicalEngineering: Code<'static> = c("128171", "Biomedical Engineering");
pub const BiometryGroup: Code<'static> = c("125005", "Biometry Group");
pub const BiophysicalProfile: Code<'static> = c("125006", "Biophysical Profile");
pub const BiopsyLocalizer: Code<'static> = c("121312", "Biopsy localizer");
pub const BiopsyShouldBeConsidered: Code<'static> = c("111143", "Biopsy should be considered");
pub const BiopsyShouldBeStronglyConsidered: Code<'static> = c("111148", "Biopsy should be strongly considered");
pub const Bioptome: Code<'static> = c("122052", "Bioptome");
pub const BiosafetyConditions: Code<'static> = c("127010", "Biosafety conditions");
pub const BiplaneEllipse: Code<'static> = c("125211", "Biplane Ellipse");
pub const BiquadFilter: Code<'static> = c("130773", "Biquad filter");
pub const Blinatumomab89Zr: Code<'static> = c("126727", "Blinatumomab ^89^Zr");
pub const BlindEstimationOfAIF: Code<'static> = c("126364", "Blind Estimation of AIF");
pub const BlindSpotMonitoring: Code<'static> = c("111844", "Blind Spot Monitoring");
pub const BlockPresent: Code<'static> = c("130458", "Block Present");
pub const BloodCarbonDioxideSaturation: Code<'static> = c("122187", "Blood Carbon dioxide saturation");
pub const BloodDeoxygenationLevel: Code<'static> = c("130823", "Blood Deoxygenation Level");
pub const BloodLabMeasurements: Code<'static> = c("122125", "Blood lab measurements");
pub const BloodOxygenContent: Code<'static> = c("122185", "Blood Oxygen content");
pub const BloodOxygenationLevel: Code<'static> = c("110819", "Blood Oxygenation Level");
pub const BloodTemperature: Code<'static> = c("122183", "Blood temperature");
pub const BloodTingedColor: Code<'static> = c("111454", "Blood tinged color");
pub const BloodVelocityMean: Code<'static> = c("122205", "Blood velocity, mean");
pub const BloodVelocityMeasurements: Code<'static> = c("122124", "Blood velocity measurements");
pub const BloodVelocityMinimum: Code<'static> = c("122206", "Blood velocity, minimum");
pub const BloodVelocityPeak: Code<'static> = c("122207", "Blood velocity, peak");
pub const BloodVelocityVsTimeOfCardiacCycle: Code<'static> = c("122667", "Blood velocity vs. time of cardiac cycle");
pub const BloodVesselVascularInvasion: Code<'static> = c("111303", "Blood vessel (vascular) invasion");
pub const BloomRichardsonGrade: Code<'static> = c("111502", "Bloom-Richardson Grade");
#[deprecated = "retired in PS3.16"]
pub const BlueFilter: Code<'static> = c("111603", "Blue filter");
#[deprecated = "retired in PS3.16"]
pub const BlueGreenFilter: Code<'static> = c("111605", "Blue-green filter");
pub const Boak: Code<'static> = c("122563", "Boak");
pub const BodeFilter: Code<'static> = c("130768", "Bode filter");
pub const BodyFrame: Code<'static> = c("130117", "Body Frame");
pub const BodyHabitusAttenuation: Code<'static> = c("122743", "Body habitus attenuation");
pub const BodyNodeSet: Code<'static> = c("130363", "Body Node Set");
pub const Bolting: Code<'static> = c("112320", "Bolting");
pub const BolusArrivalTime: Code<'static> = c("126371", "Bolus Arrival Time");
pub const BolusPresent: Code<'static> = c("130456", "Bolus Present");
pub const BolusShapingCurve: Code<'static> = c("130210", "Bolus Shaping Curve");
pub const BoneAndSoftTissue: Code<'static> = c("130407", "Bone and soft tissue");
pub const BoneMineralDensitometry: Code<'static> = c("BMD", "Bone Mineral Densitometry");
pub const BoothbyTable: Code<'static> = c("122255", "Boothby table");
pub const Bootstrap: Code<'static> = c("113217", "Bootstrap");
pub const BorderDefinition: Code<'static> = c("112007", "Border definition");
pub const BorderShape: Code<'static> = c("112015", "Border shape");
pub const Borderline: Code<'static> = c("111848", "Borderline");
pub const BorderlineAndGeneralReductionInSensitivity: Code<'static> = c("111851", "Borderline and general reduction in sensitivity");
pub const BorgCR10Scale: Code<'static> = c("122735", "Borg CR10 Scale");
pub const BorgRPEScale: Code<'static> = c("122734", "Borg RPE Scale");
pub const BostonChildrensZscoreData: Code<'static> = c("131536", "Boston Childrens Zscore Data");
pub const BottomZLocationOfReconstructableVolume: Code<'static> = c("113896", "Bottom Z Location of Reconstructable Volume");
pub const BottomZLocationOfScanningLength: Code<'static> = c("113898", "Bottom Z Location of Scanning Length");
pub const BoundaryPoint: Code<'static> = c("128141", "Boundary point");
pub const BowtieFilter: Code<'static> = c("130354", "Bowtie Filter");
pub const BrachytherapyChannel: Code<'static> = c("130080", "Brachytherapy Channel");
pub const BrachytherapyChannelShield: Code<'static> = c("130079", "Brachytherapy Channel Shield");
pub const BrachytherapyDevice: Code<'static> = c("130045", "Brachytherapy Device");
pub const BrachytherapyIsotope: Code<'static> = c("130038", "Brachytherapy Isotope");
pub const BrachytherapySourceApplicator: Code<'static> = c("130078", "Brachytherapy Source Applicator");
pub const BrandName: Code<'static> = c("111529", "Brand Name");
pub const BreakoutBoxConnected: Code<'static> = c("130891", "Breakout box connected");
pub const BreakoutBoxDisconnected: Code<'static> = c("130892", "Breakout box disconnected");
pub const BreastAttenuation: Code<'static> = c("122744", "Breast attenuation");
pub const BreastBackgroundEchotexture: Code<'static> = c("111350", "Breast background echotexture");
pub const BreastBoard: Code<'static> = c("130116", "Breast Board");
pub const BreastBridge: Code<'static> = c("130653", "Breast Bridge");
#[deprecated = "retired in PS3.16"]
pub const BreastComposition: Code<'static> = c("111006", "Breast composition");
pub const BreastFeedingHistory: Code<'static> = c("111543", "Breast feeding history");
pub const BreastGeometry: Code<'static> = c("111100", "Breast geometry");
pub const BreastImagingReport: Code<'static> = c("111400", "Breast Imaging Report");
pub const BreastImagingSubjectMatter: Code<'static> = c("128738", "Breast imaging subject matter");
pub const BreastOutlineIncludingPectoralMuscleTissue: Code<'static> = c("111007", "Breast Outline including Pectoral Muscle Tissue");
pub const BreastThickness: Code<'static> = c("128405", "Breast Thickness");
pub const BreastTissueDensity: Code<'static> = c("112191", "Breast tissue density");
pub const Brentuximab89Zr: Code<'static> = c("126735", "Brentuximab ^89^Zr");
pub const BrightRedColor: Code<'static> = c("111453", "Bright red color");
pub const BrightfieldIllumination: Code<'static> = c("111744", "Brightfield illumination");
pub const Bronchovascular: Code<'static> = c("112052", "Bronchovascular");
pub const BulletMethod: Code<'static> = c("125228", "Bullet Method");
pub const ButterflyDistribution: Code<'static> = c("112155", "Butterfly distribution");
pub const ButterflyFilter: Code<'static> = c("113652", "Butterfly filter");
pub const ButterworthFilter: Code<'static> = c("130760", "Butterworth filter");
pub const C0InadequateStudyPerAwaitingPriorComparisons: Code<'static> = c("112240", "C0 - Inadequate Study/Awaiting Prior Comparisons");
pub const C1NormalColonOrBenignLesion: Code<'static> = c("112241", "C1 - Normal Colon or Benign Lesion");
pub const C2IntermediatePolypOrIndeterminateFinding: Code<'static> = c("112242", "C2 - Intermediate Polyp or Indeterminate Finding");
pub const C3PolypPossiblyAdvancedAdenoma: Code<'static> = c("112243", "C3 - Polyp, Possibly Advanced Adenoma");
pub const C4ColonicMassLikelyMalignant: Code<'static> = c("112244", "C4 - Colonic Mass, Likely Malignant");
pub const CADOperatingPoint: Code<'static> = c("111071", "CAD Operating Point");
pub const CADOperatingPointDescription: Code<'static> = c("111081", "CAD Operating Point Description");
pub const CADOperatingPointTable: Code<'static> = c("111093", "CAD Operating Point Table");
pub const CADProcessingAndFindingsSummary: Code<'static> = c("111017", "CAD Processing and Findings Summary");
pub const CARDIOsphere: Code<'static> = c("125901", "CARDIOsphere");
pub const CArmMount: Code<'static> = c("113955", "C-Arm Mount");
pub const CCNippleNotCenteredOnImage: Code<'static> = c("111203", "CC Nipple not centered on image");
pub const CCNotAllMedialTissueVisualized: Code<'static> = c("111202", "CC Not all medial tissue visualized");
pub const CCPosteriorNippleLineDoesNotMeasureWithin1CmOfMLO: Code<'static> = c("111204", "CC Posterior nipple line does not measure within 1 cm of MLO");
pub const CD: Code<'static> = c("110032", "CD");
pub const CDImported: Code<'static> = c("110024", "CD Imported");
pub const CFXRegionInLAOProjection: Code<'static> = c("122472", "CFX Region in LAO Projection");
pub const CG250FAb289Zr: Code<'static> = c("126517", "cG250-F(ab')(2) ^89^Zr");
pub const CHARMED: Code<'static> = c("113234", "CHARMED");
pub const CICFilter: Code<'static> = c("130771", "CIC filter");
pub const CKDEPIFormulaEstimationOfGFR: Code<'static> = c("113571", "CKD-EPI Formula estimation of GFR");
pub const CKMBBaseline: Code<'static> = c("122180", "CK-MB baseline");
pub const CKMBPeak: Code<'static> = c("122181", "CK-MB peak");
pub const CLR1404I124: Code<'static> = c("126715", "CLR1404 I^124^");
pub const CLR1404I131: Code<'static> = c("126716", "CLR1404 I^131^");
pub const CMAbU3689Zr: Code<'static> = c("126746", "cMAb U36 ^89^Zr");
pub const CO2Respiration: Code<'static> = c("130443", "CO2 Respiration");
pub const CO2Transcutaneous: Code<'static> = c("130434", "CO2 Transcutaneous");
pub const CO2TrendEndTidalMainStream: Code<'static> = c("130436", "CO2 Trend End-tidal Main-stream");
pub const CO2TrendEndTidalSideStream: Code<'static> = c("130438", "CO2 Trend End-tidal Side-stream");
pub const CO2TrendMainStream: Code<'static> = c("130441", "CO2 Trend Main-stream");
pub const CO2TrendSideStream: Code<'static> = c("130442", "CO2 Trend Side-stream");
pub const CO2WaveformEndTidalMainStream: Code<'static> = c("130435", "CO2 Waveform End-tidal Main-stream");
pub const CO2WaveformEndTidalSideStream: Code<'static> = c("130437", "CO2 Waveform End-tidal Side-stream");
pub const CO2WaveformMainStream: Code<'static> = c("130439", "CO2 Waveform Main-stream");
pub const CO2WaveformSideStream: Code<'static> = c("130440", "CO2 Waveform Side-stream");
pub const CRPerDRMechanicalConfiguration: Code<'static> = c("113956", "CR/DR Mechanical Configuration");
pub const CRTDisplay: Code<'static> = c("109991", "CRT Display");
pub const CRTFrontProjectionSystem: Code<'static> = c("109998", "CRT Front Projection System");
pub const CRTRearProjectionSystem: Code<'static> = c("109997", "CRT Rear Projection System");
pub const CTAccumulatedDoseData: Code<'static> = c("113811", "CT Accumulated Dose Data");
pub const CTAcquisition: Code<'static> = c("113819", "CT Acquisition");
pub const CTAcquisitionParameters: Code<'static> = c("113822", "CT Acquisition Parameters");
pub const CTAcquisitionType: Code<'static> = c("113820", "CT Acquisition Type");
pub const CTBasedAttenuationCorrection: Code<'static> = c("122728", "CT-based attenuation correction");
pub const CTDIfreeairCalculationFactor: Code<'static> = c("113836", "CTDIfreeair Calculation Factor");
pub const CTDIfreeairToEConversionViaMCComputation: Code<'static> = c("113801", "CTDIfreeair to E conversion via MC computation");
pub const CTDIfreeairToEConversionViaMeasurement: Code<'static> = c("113803", "CTDIfreeair to E conversion via measurement");
pub const CTDIvolAlertValue: Code<'static> = c("113904", "CTDIvol Alert Value");
pub const CTDIvolAlertValueConfigured: Code<'static> = c("113902", "CTDIvol Alert Value Configured");
pub const CTDIvolForwardEstimate: Code<'static> = c("113914", "CTDIvol Forward Estimate");
pub const CTDIvolNotificationValue: Code<'static> = c("113912", "CTDIvol Notification Value");
pub const CTDIvolNotificationValueConfigured: Code<'static> = c("113910", "CTDIvol Notification Value Configured");
pub const CTDIwPhantomType: Code<'static> = c("113835", "CTDIw Phantom Type");
pub const CTDose: Code<'static> = c("113829", "CT Dose");
pub const CTDoseLengthProductSubTotal: Code<'static> = c("130745", "CT Dose Length Product Sub-Total");
pub const CTDoseLengthProductTotal: Code<'static> = c("113813", "CT Dose Length Product Total");
pub const CTEffectiveDoseTotal: Code<'static> = c("113814", "CT Effective Dose Total");
pub const CTImageReconstruction: Code<'static> = c("110015", "CT Image Reconstruction");
pub const CTProtocol: Code<'static> = c("CTPROTOCOL", "CT Protocol");
pub const CTVNodal: Code<'static> = c("130049", "CTV Nodal");
pub const CTVPrimary: Code<'static> = c("130050", "CTV Primary");
pub const CTXRaySourceParameters: Code<'static> = c("113831", "CT X-Ray Source Parameters");
pub const CU3689Zr: Code<'static> = c("126515", "cU36 ^89^Zr");
pub const CWavePressure: Code<'static> = c("122196", "C wave pressure");
#[deprecated = "retired in PS3.16"]
pub const CalcificationCluster: Code<'static> = c("111105", "Calcification Cluster");
pub const CalcificationDescriptor: Code<'static> = c("112030", "Calcification Descriptor");
pub const CalcificationDistribution: Code<'static> = c("111008", "Calcification Distribution");
pub const CalcificationExtentAsPercentOfSurface: Code<'static> = c("112018", "Calcification extent as percent of surface");
pub const CalcificationExtentAsPercentOfVolume: Code<'static> = c("112019", "Calcification extent as percent of volume");
pub const CalcificationType: Code<'static> = c("111009", "Calcification Type");
pub const CalcificationsOutsideOfAMass: Code<'static> = c("111347", "Calcifications outside of a mass");
pub const CalcificationsSeenInTheCore: Code<'static> = c("111446", "Calcifications seen in the core");
pub const CalcificationsWithinAMass: Code<'static> = c("111346", "Calcifications within a mass");
pub const CalcifiedImplant: Code<'static> = c("111505", "Calcified implant");
pub const Calcium: Code<'static> = c("112145", "Calcium");
pub const CalciumLength: Code<'static> = c("122341", "Calcium Length");
pub const CalciumMass: Code<'static> = c("122661", "Calcium Mass");
pub const CalciumMassThreshold: Code<'static> = c("122658", "Calcium Mass Threshold");
#[deprecated = "retired in PS3.16"]
pub const CalciumScore: Code<'static> = c("112058", "Calcium score");
pub const CalciumScoringAnalysis: Code<'static> = c("122603", "Calcium Scoring Analysis");
pub const CalciumScoringCalibration: Code<'static> = c("122659", "Calcium Scoring Calibration");
pub const CalciumVolume: Code<'static> = c("122660", "Calcium Volume");
pub const Calculated: Code<'static> = c("125315", "Calculated");
#[deprecated = "retired in PS3.16"]
pub const Calculated_121428: Code<'static> = c("121428", "Calculated");
pub const CalculationDescription: Code<'static> = c("112034", "Calculation Description");
pub const CalculationFailure: Code<'static> = c("114008", "Calculation failure");
pub const Calibration: Code<'static> = c("122505", "Calibration");
pub const CalibrationDateTime: Code<'static> = c("113723", "Calibration DateTime");
pub const CalibrationFactor: Code<'static> = c("122322", "Calibration Factor");
pub const CalibrationIntendedAcquisitionProtocol: Code<'static> = c("131331", "Calibration Intended Acquisition Protocol");
pub const CalibrationIntendedDoseIndex: Code<'static> = c("131330", "Calibration Intended Dose Index");
pub const CalibrationMethod: Code<'static> = c("122422", "Calibration Method");
pub const CalibrationObject: Code<'static> = c("122421", "Calibration Object");
pub const CalibrationObjectSize: Code<'static> = c("122423", "Calibration Object Size");
pub const CalibrationObjectUsed: Code<'static> = c("122488", "Calibration Object Used");
pub const CalibrationProtocol: Code<'static> = c("113720", "Calibration Protocol");
pub const CalibrationResponsibleParty: Code<'static> = c("113724", "Calibration Responsible Party");
pub const CalibrationUncertainty: Code<'static> = c("113763", "Calibration Uncertainty");
pub const CamellinCalossi: Code<'static> = c("111875", "Camellin-Calossi");
pub const CanadianBorderCollieAssociation: Code<'static> = c("109207", "Canadian Border Collie Association");
pub const CanadianKennelClub: Code<'static> = c("109208", "Canadian Kennel Club");
pub const CanadianLivestockRecordsAssociation: Code<'static> = c("109209", "Canadian Livestock Records Association");
pub const CancerElsewhere: Code<'static> = c("111480", "Cancer elsewhere");
pub const CanineFederationOfCanada: Code<'static> = c("109210", "Canine Federation of Canada");
pub const CarcinomaInChildren: Code<'static> = c("111304", "Carcinoma in children");
pub const CarcinomaInEctopicBreast: Code<'static> = c("111305", "Carcinoma in ectopic breast");
pub const CarcinomaInPregnancyAndLactation: Code<'static> = c("111310", "Carcinoma in pregnancy and lactation");
pub const CarcinomaWithEndocrineDifferentiation: Code<'static> = c("111306", "Carcinoma with endocrine differentiation");
#[deprecated = "retired in PS3.16"]
pub const CarcinomaWithMetaplasia: Code<'static> = c("111308", "Carcinoma with metaplasia");
#[deprecated = "retired in PS3.16"]
pub const Carcinosarcoma: Code<'static> = c("111311", "Carcinosarcoma");
pub const CardiacAxis: Code<'static> = c("131374", "Cardiac axis");
pub const CardiacAxisAngle: Code<'static> = c("131350", "Cardiac Axis Angle");
pub const CardiacElectrophysiology: Code<'static> = c("EPS", "Cardiac Electrophysiology");
pub const CardiacFunctionScore: Code<'static> = c("131033", "Cardiac Function Score");
pub const CardiacImagingSpecialty: Code<'static> = c("128007", "Cardiac Imaging Specialty");
#[deprecated = "retired in PS3.16"]
pub const CardiacStressState: Code<'static> = c("109091", "Cardiac Stress State");
pub const CardiothoracicSizeRatioScore: Code<'static> = c("131032", "Cardiothoracic Size Ratio Score");
pub const CardiovascularAnalysisReport: Code<'static> = c("122600", "Cardiovascular Analysis Report");
pub const CardiovascularImagingSubjectMatter: Code<'static> = c("128730", "Cardiovascular imaging subject matter");
pub const CardiovascularRadiology: Code<'static> = c("128172", "Cardiovascular Radiology");
pub const CarinaAngle: Code<'static> = c("112086", "Carina angle");
pub const CarrierGas: Code<'static> = c("127330", "Carrier gas");
pub const CarrierTemperatureSensor: Code<'static> = c("127240", "Carrier temperature sensor");
pub const CartilaginousAndOsseousChange: Code<'static> = c("111309", "Cartilaginous and osseous change");
pub const CaseSensitivity: Code<'static> = c("111088", "Case Sensitivity");
pub const CaseSpecificity: Code<'static> = c("111090", "Case Specificity");
pub const CassetteBasedProjectionRadiographySystem: Code<'static> = c("113959", "Cassette-based Projection Radiography System");
pub const Cast: Code<'static> = c("130114", "Cast");
pub const CathLabProcedureLog: Code<'static> = c("121120", "Cath Lab Procedure Log");
pub const CatheterCurve: Code<'static> = c("122097", "Catheter Curve");
pub const CatheterSize: Code<'static> = c("122319", "Catheter Size");
#[deprecated = "retired in PS3.16"]
pub const CatheterizationProcedurePhase: Code<'static> = c("109057", "Catheterization Procedure Phase");
pub const Caudal10DegreeDistalCranioproximalOblique: Code<'static> = c("123019", "Caudal 10 degree distal-cranioproximal oblique");
pub const CavityExtentAsPercentOfVolume: Code<'static> = c("112017", "Cavity extent as percent of volume");
pub const CavityRadiationShield: Code<'static> = c("130640", "Cavity radiation shield");
pub const Center: Code<'static> = c("111010", "Center");
pub const CenterOfGravity: Code<'static> = c("122475", "Center of Gravity");
pub const CenterOfMass: Code<'static> = c("128138", "Center of Mass");
pub const CenterOfRotation: Code<'static> = c("130521", "Center of Rotation");
pub const CenterOfTableHead: Code<'static> = c("128751", "Center of Table Head");
pub const Centerline: Code<'static> = c("130490", "Centerline");
pub const CenterlineWallMotionAnalysis: Code<'static> = c("122449", "Centerline Wall Motion Analysis");
pub const CentralBreathingPosition: Code<'static> = c("122612", "Central breathing position");
pub const CentralLine: Code<'static> = c("112174", "Central line");
pub const Centrilobular: Code<'static> = c("112156", "Centrilobular");
pub const CentrilobularStructures: Code<'static> = c("112087", "Centrilobular structures");
pub const CerebroplacentalRatio: Code<'static> = c("131009", "Cerebroplacental ratio");
pub const CertaintyOfFeature: Code<'static> = c("111011", "Certainty of Feature");
pub const CertaintyOfFinding: Code<'static> = c("111012", "Certainty of Finding");
pub const CertaintyOfImpression: Code<'static> = c("111013", "Certainty of Impression");
pub const CessationOfExercise: Code<'static> = c("125237", "Cessation of exercise");
pub const CessationOfStimulation: Code<'static> = c("125239", "Cessation of stimulation");
pub const Cetuximab89Zr: Code<'static> = c("126513", "Cetuximab ^89^Zr");
pub const ChairOfProtocolCommittee: Code<'static> = c("128671", "Chair of Protocol Committee");
pub const ChangMethod: Code<'static> = c("122721", "Chang method");
pub const ChangeInPatientAnatomy: Code<'static> = c("130464", "Change in Patient Anatomy");
pub const ChangeOfProcedureForCorrectCharging: Code<'static> = c("110509", "Change of procedure for correct charging");
pub const ChannelObstruction: Code<'static> = c("110520", "Channel Obstruction");
pub const ChebyshevFilter: Code<'static> = c("130761", "Chebyshev filter");
pub const CheckedInStatus: Code<'static> = c("121388", "Checked-In Status");
pub const Checkerboard: Code<'static> = c("114216", "Checkerboard");
pub const ChestCADReport: Code<'static> = c("112000", "Chest CAD Report");
pub const ChestTube: Code<'static> = c("112173", "Chest tube");
pub const ChiSquare: Code<'static> = c("126221", "Chi-square");
pub const CholineC11: Code<'static> = c("126703", "Choline C^11^");
pub const CholinePerCreatineRatio: Code<'static> = c("113081", "Choline/Creatine Ratio");
pub const ChoriocapillarisStructuralReflectanceMap: Code<'static> = c("128274", "Choriocapillaris structural reflectance map");
pub const ChoriocapillarisVasculatureFlow: Code<'static> = c("128273", "Choriocapillaris vasculature flow");
pub const ChoroidStructuralReflectanceMap: Code<'static> = c("128276", "Choroid structural reflectance map");
pub const ChoroidVasculatureFlow: Code<'static> = c("128275", "Choroid vasculature flow");
pub const ChromaticityEvaluation: Code<'static> = c("109705", "Chromaticity evaluation");
pub const CineFilmDigitized: Code<'static> = c("110021", "Cine Film Digitized");
#[deprecated = "retired in PS3.16"]
pub const Cinefluorography: Code<'static> = c("CF", "Cinefluorography");
pub const CircadianEffects: Code<'static> = c("127050", "Circadian effects");
pub const CircleBScanPattern: Code<'static> = c("128284", "Circle B-scan pattern");
pub const CircleRadialBScanPattern: Code<'static> = c("128287", "Circle-radial B-scan pattern");
pub const CircleRasterBScanPattern: Code<'static> = c("128286", "Circle-raster B-scan pattern");
pub const CircularMethod: Code<'static> = c("122473", "Circular Method");
#[deprecated = "retired in PS3.16"]
pub const Circulating: Code<'static> = c("121100", "Circulating");
pub const CirculatorySupport: Code<'static> = c("122138", "Circulatory Support");
pub const CircumferentialLagrangianStrain: Code<'static> = c("130677", "Circumferential Lagrangian Strain");
pub const CircumferentialLagrangianStrainRate: Code<'static> = c("130678", "Circumferential Lagrangian Strain Rate");
pub const CircumpapillaryRetinalNerveFiberLayerKeyMeasurements: Code<'static> = c("131242", "Circumpapillary Retinal Nerve Fiber Layer Key Measurements");
#[deprecated = "retired in PS3.16"]
pub const Circumscribed: Code<'static> = c("112142", "Circumscribed");
pub const ClassActivation: Code<'static> = c("130402", "Class activation");
pub const CleanDescriptorsOption: Code<'static> = c("113105", "Clean Descriptors Option");
pub const CleanGraphicsOption: Code<'static> = c("113103", "Clean Graphics Option");
pub const CleanPixelDataOption: Code<'static> = c("113101", "Clean Pixel Data Option");
pub const CleanRecognizableVisualFeaturesOption: Code<'static> = c("113102", "Clean Recognizable Visual Features Option");
pub const CleanStructuredContentOption: Code<'static> = c("113104", "Clean Structured Content Option");
pub const ClearanceInterlock: Code<'static> = c("130739", "Clearance Interlock");
pub const ClinicalContext: Code<'static> = c("122127", "Clinical Context");
pub const ClinicalEvaluation: Code<'static> = c("109710", "Clinical evaluation");
pub const ClinicalFinding: Code<'static> = c("111402", "Clinical Finding");
pub const ClinicalFractionNumber: Code<'static> = c("121390", "Clinical Fraction Number");
pub const ClinicalInterpretation: Code<'static> = c("122147", "Clinical Interpretation");
pub const ClinicalTrialSubmissionInputUsed: Code<'static> = c("128223", "Clinical Trial Submission Input Used");
pub const ClipboardManager: Code<'static> = c("110158", "Clipboard Manager");
pub const ClockfaceOrRegion: Code<'static> = c("111014", "Clockface or region");
pub const ClusterAnalysis: Code<'static> = c("123107", "Cluster Analysis");
#[deprecated = "retired in PS3.16"]
pub const ClusterProminenceOfGLCM: Code<'static> = c("128797", "Cluster Prominence of GLCM");
#[deprecated = "retired in PS3.16"]
pub const ClusterShadeOfGLCM: Code<'static> = c("128796", "Cluster Shade of GLCM");
#[deprecated = "retired in PS3.16"]
pub const ClusterTendencyOfGLCM: Code<'static> = c("128795", "Cluster Tendency of GLCM");
pub const ClusteredMicrocysts: Code<'static> = c("111129", "Clustered microcysts");
pub const CoachingDevice: Code<'static> = c("130650", "Coaching Device");
pub const Coalescent: Code<'static> = c("112157", "Coalescent");
pub const CockroftGaultFormulaEstimationOfGFR: Code<'static> = c("113570", "Cockroft-Gault Formula estimation of GFR");
pub const CoilMarker: Code<'static> = c("129301", "Coil Marker");
pub const CoilPlacementConcern: Code<'static> = c("130571", "Coil placement concern");
pub const CoilSelectionConcern: Code<'static> = c("130572", "Coil selection concern");
pub const Coin: Code<'static> = c("112178", "Coin");
pub const CollectionOfPresentationStates: Code<'static> = c("113022", "Collection of Presentation States");
pub const CollimatedFieldArea: Code<'static> = c("113790", "Collimated Field Area");
pub const CollimatedFieldHeight: Code<'static> = c("113788", "Collimated Field Height");
pub const CollimatedFieldWidth: Code<'static> = c("113789", "Collimated Field Width");
pub const CollimationTooCloseToBreast: Code<'static> = c("111195", "Collimation too close to breast");
#[deprecated = "retired in PS3.16"]
pub const Collimator: Code<'static> = c("111173", "Collimator");
pub const CollisionChecks: Code<'static> = c("130652", "Collision checks");
pub const ColonCADReport: Code<'static> = c("112220", "Colon CAD Report");
pub const ColonOverallAssessment: Code<'static> = c("112222", "Colon Overall Assessment");
#[deprecated = "retired in PS3.16"]
pub const ColorFlowDoppler: Code<'static> = c("CD", "Color flow Doppler");
pub const ColumnAngulation: Code<'static> = c("113770", "Column Angulation");
pub const CombinationImplant: Code<'static> = c("111484", "Combination implant");
pub const CombinedCardiacOutput: Code<'static> = c("131054", "Combined Cardiac Output");
pub const CombinedPosteriorEnhancementAndShadowing: Code<'static> = c("111370", "Combined posterior enhancement and shadowing");
pub const CombinedStructuredLightPerThermalImager: Code<'static> = c("130648", "Combined Structured Light/Thermal Imager");
pub const Comment: Code<'static> = c("121106", "Comment");
pub const CompactFlash: Code<'static> = c("110034", "Compact Flash");
pub const ComparativeAverage10YearCHDRisk: Code<'static> = c("122231", "Comparative Average10 Year CHD Risk");
pub const ComparativeLow10YearCHDRisk: Code<'static> = c("122232", "Comparative Low10 Year CHD Risk");
pub const ComparisonToPreviousStudies: Code<'static> = c("111424", "Comparison to previous studies");
pub const ComparisonWithPriorStudyDone: Code<'static> = c("122140", "Comparison with Prior Study Done");
pub const CompleteAcquisitionContent: Code<'static> = c("113034", "Complete Acquisition Content");
pub const CompleteRenderingForPresentation: Code<'static> = c("121332", "Complete Rendering for Presentation");
pub const CompleteStudyContent: Code<'static> = c("113032", "Complete Study Content");
pub const CompletelyEncapsulated: Code<'static> = c("130594", "Completely encapsulated");
pub const Complex: Code<'static> = c("111363", "Complex");
pub const ComplexCyst: Code<'static> = c("111460", "Complex cyst");
pub const ComplicatedCyst: Code<'static> = c("111130", "Complicated cyst");
#[deprecated = "retired in PS3.16"]
pub const Complications: Code<'static> = c("121113", "Complications");
pub const ComponentConnection: Code<'static> = c("112350", "Component Connection");
pub const ComponentID: Code<'static> = c("112347", "Component ID");
pub const ComponentModel: Code<'static> = c("129011", "Component Model");
pub const ComponentType: Code<'static> = c("112370", "Component Type");
pub const ComponentVolume: Code<'static> = c("130239", "Component Volume");
pub const ComposedFromDeformableRegistration: Code<'static> = c("121367", "Composed from deformable registration");
pub const ComposedFromImageSeriesOtherThanPlanningImageSeries: Code<'static> = c("121368", "Composed from image series other than planning image series");
pub const ComposedFromPriorDoses: Code<'static> = c("121370", "Composed from prior doses");
pub const ComposedFromPriorDosesAndCurrentPlan: Code<'static> = c("121371", "Composed from prior doses and current plan");
pub const ComposedFromSetupPerturbation: Code<'static> = c("121369", "Composed from setup perturbation");
pub const ComposedWithRadiobiologicalEffects: Code<'static> = c("121377", "Composed with radiobiological effects");
pub const ComposedWithWeightingForFractionsDelivered: Code<'static> = c("121378", "Composed with weighting for fractions delivered");
pub const CompositeFeature: Code<'static> = c("111015", "Composite Feature");
pub const CompositeFeatureModifier: Code<'static> = c("112023", "Composite Feature Modifier");
pub const CompositeType: Code<'static> = c("111016", "Composite type");
pub const CompoundStatement: Code<'static> = c("122150", "Compound Statement");
pub const CompressionContactArea: Code<'static> = c("111649", "Compression Contact Area");
pub const CompressionForce: Code<'static> = c("111647", "Compression Force");
pub const CompressionPressure: Code<'static> = c("111648", "Compression Pressure");
pub const CompressionThickness: Code<'static> = c("111633", "Compression Thickness");
pub const ComptonScattering: Code<'static> = c("131568", "Compton Scattering");
pub const ComputationServer: Code<'static> = c("COMP", "Computation Server");
pub const ComputedFromImageAttributes: Code<'static> = c("113867", "Computed From Image Attributes");
pub const ComputedRadiography: Code<'static> = c("CR", "Computed Radiography");
pub const ComputedTomography: Code<'static> = c("CT", "Computed Tomography");
pub const ComputerAidedDetection: Code<'static> = c("110004", "Computer Aided Detection");
pub const ComputerAidedDiagnosis: Code<'static> = c("110003", "Computer Aided Diagnosis");
pub const ComputerAssistedDetectionPerDiagnosis: Code<'static> = c("CAD", "Computer Assisted Detection/Diagnosis");
pub const Concentration: Code<'static> = c("122093", "Concentration");
pub const ConcentricCircleBScanPattern: Code<'static> = c("128285", "Concentric circle B-scan pattern");
pub const Concern: Code<'static> = c("121430", "Concern");
pub const Conclusion: Code<'static> = c("121077", "Conclusion");
#[deprecated = "retired in PS3.16"]
pub const Conclusions: Code<'static> = c("121076", "Conclusions");
pub const ConditionEffectiveDoseMeasured: Code<'static> = c("113816", "Condition Effective Dose measured");
pub const ConePresent: Code<'static> = c("130457", "Cone Present");
pub const ConfiguredDefaultImagingEnergy: Code<'static> = c("130807", "Configured Default Imaging Energy");
pub const ConfiguredLowestImagingEnergy: Code<'static> = c("130806", "Configured Lowest Imaging Energy");
pub const ConfirmationOfTarget: Code<'static> = c("111442", "Confirmation of target");
pub const ConfocalImaging: Code<'static> = c("114207", "Confocal imaging");
pub const ConfocalMicroscopy: Code<'static> = c("CFM", "Confocal Microscopy");
pub const ConformalArcBeam: Code<'static> = c("130104", "Conformal Arc Beam");
pub const ConformalityShell: Code<'static> = c("130062", "Conformality Shell");
pub const ConjunctiveTerm: Code<'static> = c("122154", "Conjunctive Term");
pub const ConnectedImplantationPlanComponent: Code<'static> = c("112374", "Connected Implantation Plan Component");
pub const ConsistentWithLabelingOfTheDevice: Code<'static> = c("128602", "Consistent with labeling of the device");
pub const ConstantAngleAcquisition: Code<'static> = c("113805", "Constant Angle Acquisition");
pub const ConsultationWith: Code<'static> = c("122044", "Consultation With");
pub const ConsumableCatheterType: Code<'static> = c("130257", "Consumable Catheter Type");
pub const ConsumableIsNew: Code<'static> = c("130224", "Consumable is New");
pub const ConsumableReturnedToInventory: Code<'static> = c("122077", "Consumable returned to inventory");
pub const ConsumableTakenFromInventory: Code<'static> = c("122076", "Consumable taken from inventory");
pub const ConsumableUnusable: Code<'static> = c("122079", "Consumable unusable");
pub const ContentAssessmentResult: Code<'static> = c("ASMT", "Content Assessment Result");
pub const ContentDate: Code<'static> = c("111018", "Content Date");
pub const ContentTime: Code<'static> = c("111019", "Content Time");
pub const ContinentalKennelClub: Code<'static> = c("109211", "Continental Kennel Club");
pub const ContinuityEquation: Code<'static> = c("125212", "Continuity Equation");
pub const ContinuityEquationByMeanVelocity: Code<'static> = c("125213", "Continuity Equation by Mean Velocity");
pub const ContinuityEquationByPeakVelocity: Code<'static> = c("125214", "Continuity Equation by Peak Velocity");
pub const ContinuityEquationByVelocityTimeIntegral: Code<'static> = c("125215", "Continuity Equation by Velocity Time Integral");
pub const Continuous: Code<'static> = c("113630", "Continuous");
pub const ContinuousPermanent: Code<'static> = c("130101", "Continuous Permanent");
pub const ContinuousTemporary: Code<'static> = c("130100", "Continuous Temporary");
pub const ContourEnd: Code<'static> = c("122482", "Contour End");
pub const ContourRealignment: Code<'static> = c("122410", "Contour Realignment");
pub const ContourStart: Code<'static> = c("122481", "Contour Start");
pub const ContouringInputUsed: Code<'static> = c("128219", "Contouring Input Used");
pub const ContrastAdministered: Code<'static> = c("122086", "Contrast administered");
pub const ContrastAgentAngioMRSignalIntensity: Code<'static> = c("110801", "Contrast Agent Angio MR Signal Intensity");
pub const ContrastEnd: Code<'static> = c("122085", "Contrast end");
pub const ContrastLongitudinalRelaxivity: Code<'static> = c("126380", "Contrast Longitudinal Relaxivity");
#[deprecated = "retired in PS3.16"]
pub const ContrastOfGLCM: Code<'static> = c("126063", "Contrast of GLCM");
pub const ContrastPerBolusAgent: Code<'static> = c("123011", "Contrast/Bolus Agent");
pub const ContrastPerBolusIngredientOpaque: Code<'static> = c("121381", "Contrast/Bolus Ingredient Opaque");
pub const ContrastPhase: Code<'static> = c("109058", "Contrast Phase");
pub const ContrastReactionProphylacticAgent: Code<'static> = c("130259", "Contrast Reaction Prophylactic Agent");
pub const ContrastStart: Code<'static> = c("122084", "Contrast start");
pub const ContrastTransverseRelaxivity: Code<'static> = c("130188", "Contrast Transverse Relaxivity");
pub const ContrastVolumeLimit: Code<'static> = c("130228", "Contrast Volume Limit");
pub const ControlledSSDSetupMethod: Code<'static> = c("130631", "Controlled SSD Setup Method");
pub const ConversionFactor: Code<'static> = c("128528", "Conversion Factor");
pub const ConvolutionSuperposition: Code<'static> = c("131324", "Convolution Superposition");
pub const CoopersLigamentChanges: Code<'static> = c("111111", "Cooper's ligament changes");
pub const CoordinateAlignmentImageSeries: Code<'static> = c("128187", "Coordinate Alignment Image Series");
pub const CopiedFromImageAttributes: Code<'static> = c("113866", "Copied From Image Attributes");
pub const CornCobBedding: Code<'static> = c("127232", "Corn cob bedding");
pub const CornealAxialPowerMap: Code<'static> = c("111940", "Corneal axial power map");
pub const CornealBirefringenceCompensation: Code<'static> = c("111923", "Corneal birefringence compensation");
pub const CornealElevationMap: Code<'static> = c("111943", "Corneal elevation map");
pub const CornealInstantaneousPowerMap: Code<'static> = c("111941", "Corneal instantaneous power map");
pub const CornealRefractivePowerMap: Code<'static> = c("111942", "Corneal refractive power map");
pub const CornealWavefrontMap: Code<'static> = c("111944", "Corneal wavefront map");
pub const CoronaRadiata: Code<'static> = c("112105", "Corona radiata");
pub const CoronaryArteryBypassDuringThisAdmission: Code<'static> = c("122164", "Coronary Artery Bypass During This Admission");
pub const CoronaryArteryDiseaseAssessment: Code<'static> = c("130720", "Coronary Artery Disease Assessment");
pub const CoronaryArteryDiseaseAssessmentModifier: Code<'static> = c("130721", "Coronary Artery Disease Assessment Modifier");
pub const CoronaryArteryDiseaseStenosisAssessmentModifier: Code<'static> = c("130722", "Coronary Artery Disease Stenosis Assessment Modifier");
pub const CoronaryFlowReserve: Code<'static> = c("122217", "Coronary Flow reserve");
pub const CoronaryLesionGreaterThanEquals50PercentStenosis: Code<'static> = c("122171", "Coronary lesion > = 50% stenosis");
pub const CorrectedSinusNodeRecoveryTime: Code<'static> = c("122237", "Corrected Sinus Node Recovery Time");
pub const CorrectionFactor: Code<'static> = c("128452", "Correction Factor");
#[deprecated = "retired in PS3.16"]
pub const CorrectionOfCongenitalCardiovascularDeformity: Code<'static> = c("121439", "Correction of congenital cardiovascular deformity");
pub const CorrelatesToMRIFindings: Code<'static> = c("111383", "Correlates to MRI findings");
pub const CorrelatesToMammographyFindings: Code<'static> = c("111382", "Correlates to mammography findings");
pub const CorrelatesToOtherImagingFindings: Code<'static> = c("111385", "Correlates to other imaging findings");
pub const CorrelatesToPhysicalExaminationFindings: Code<'static> = c("111381", "Correlates to physical examination findings");
pub const CorrelatesToUltrasoundFindings: Code<'static> = c("111384", "Correlates to ultrasound findings");
#[deprecated = "retired in PS3.16"]
pub const CorrelationOfGLCM: Code<'static> = c("126066", "Correlation of GLCM");
pub const CorrelationToOtherFindings: Code<'static> = c("111380", "Correlation to other Findings");
pub const CouchIndexLabel: Code<'static> = c("130657", "Couch Index Label");
pub const CountryOfLanguage: Code<'static> = c("121046", "Country of Language");
pub const CoverSlipDefect: Code<'static> = c("131505", "Cover slip defect");
pub const CoverslipEdge: Code<'static> = c("131510", "Coverslip edge");
pub const CreatineAndCholine: Code<'static> = c("113094", "Creatine and Choline");
pub const CreatinePlusCholinePerCitrateRatio: Code<'static> = c("113096", "Creatine+Choline/Citrate Ratio");
pub const CrossBScanPattern: Code<'static> = c("128283", "Cross B-scan pattern");
pub const CrossSectionalScanPlaneOrientation: Code<'static> = c("130546", "Cross-sectional scan plane orientation");
pub const CrossedGrid: Code<'static> = c("111645", "Crossed grid");
pub const Crosstable: Code<'static> = c("111069", "Crosstable");
pub const CubeBScanPattern: Code<'static> = c("128279", "Cube B-scan pattern");
pub const CubeMethod: Code<'static> = c("125206", "Cube Method");
#[deprecated = "retired in PS3.16"]
pub const Culposcopy: Code<'static> = c("CP", "Culposcopy");
pub const CupToDiscAreaRatio: Code<'static> = c("131256", "Cup to disc area ratio");
pub const CupToDiscRatioHorizontal: Code<'static> = c("131258", "Cup to disc ratio horizontal");
pub const CupToDiscRatioVertical: Code<'static> = c("131257", "Cup to disc ratio vertical");
pub const CurrentFractionNumber: Code<'static> = c("121385", "Current Fraction Number");
pub const CurrentPregnancyKnownOrSuspectedMalformationsPerSyndromes: Code<'static> = c("111573", "Current pregnancy, known or suspected malformations/syndromes");
#[deprecated = "retired in PS3.16"]
pub const CurrentProcedureDescriptions: Code<'static> = c("121064", "Current Procedure Descriptions");
pub const CurrentProcedureEvidence: Code<'static> = c("122073", "Current procedure evidence");
pub const CurrentResponse: Code<'static> = c("112048", "Current Response");
pub const CurveFitMethod: Code<'static> = c("122627", "Curve Fit Method");
pub const CurveFitParameter: Code<'static> = c("128453", "Curve Fit Parameter");
pub const CurveFittedReference: Code<'static> = c("122489", "Curve Fitted Reference");
pub const CurvedLinearUltrasoundTransducerGeometry: Code<'static> = c("125253", "Curved linear ultrasound transducer geometry");
pub const CurvedMultiplanarReformatting: Code<'static> = c("113073", "Curved multiplanar reformatting");
pub const CutOffTissue: Code<'static> = c("131509", "Cut off tissue");
pub const CylinderMarker: Code<'static> = c("129303", "Cylinder Marker");
pub const CystFill: Code<'static> = c("111430", "Cyst fill");
#[deprecated = "retired in PS3.16"]
pub const Cystoscopy: Code<'static> = c("CS", "Cystoscopy");
pub const CytologicAnalysis: Code<'static> = c("111147", "Cytologic analysis");
pub const DICOMInstancesAccessed: Code<'static> = c("110103", "DICOM Instances Accessed");
pub const DICOMInstancesTransferred: Code<'static> = c("110104", "DICOM Instances Transferred");
pub const DICOMObjectCatalog: Code<'static> = c("121181", "DICOM Object Catalog");
pub const DICOMStudyDeleted: Code<'static> = c("110105", "DICOM Study Deleted");
#[deprecated = "retired in PS3.16"]
pub const DINGeometryPattern: Code<'static> = c("109932", "DIN Geometry Pattern");
#[deprecated = "retired in PS3.16"]
pub const DINGrayscalePattern: Code<'static> = c("109931", "DIN Grayscale Pattern");
#[deprecated = "retired in PS3.16"]
pub const DINResolutionPattern: Code<'static> = c("109933", "DIN Resolution Pattern");
pub const DKI: Code<'static> = c("113222", "DKI");
pub const DLP: Code<'static> = c("113838", "DLP");
pub const DLPAlertValue: Code<'static> = c("113903", "DLP Alert Value");
pub const DLPAlertValueConfigured: Code<'static> = c("113901", "DLP Alert Value Configured");
pub const DLPForwardEstimate: Code<'static> = c("113913", "DLP Forward Estimate");
pub const DLPFrontProjectionSystem: Code<'static> = c("109996", "DLP Front Projection System");
pub const DLPNotificationValue: Code<'static> = c("113911", "DLP Notification Value");
pub const DLPNotificationValueConfigured: Code<'static> = c("113909", "DLP Notification Value Configured");
pub const DLPRearProjectionSystem: Code<'static> = c("109995", "DLP Rear Projection System");
pub const DLPToEConversionViaMCComputation: Code<'static> = c("113800", "DLP to E conversion via MC computation");
pub const DLPToEConversionViaMeasurement: Code<'static> = c("113802", "DLP to E conversion via measurement");
pub const DN3089Zr: Code<'static> = c("126747", "DN30 ^89^Zr");
pub const DOT: Code<'static> = c("113236", "DOT");
pub const DPA71311C: Code<'static> = c("126765", "DPA-713 ^11^C");
pub const DPA71418F: Code<'static> = c("126766", "DPA-714 ^18^F");
pub const DSI: Code<'static> = c("113224", "DSI");
pub const DTI: Code<'static> = c("113223", "DTI");
pub const DVAWavePeakVelocity: Code<'static> = c("131352", "DV a-wave peak velocity");
pub const DVD: Code<'static> = c("110033", "DVD");
pub const DVDImported: Code<'static> = c("110025", "DVD Imported");
pub const DVPeakVelocityIndexInVeins: Code<'static> = c("131015", "DV Peak Velocity Index in Veins");
pub const DVPreloadIndex: Code<'static> = c("131353", "DV preload index");
pub const DVPulsatilityIndexInVeins: Code<'static> = c("131014", "DV Pulsatility Index in Veins");
pub const DVSPerA: Code<'static> = c("131354", "DV S/a");
pub const DVSWavePeakVelocity: Code<'static> = c("131351", "DV S-wave peak velocity");
pub const DW: Code<'static> = c("126222", "D-W");
pub const DarkBrownColor: Code<'static> = c("111452", "Dark brown color");
pub const DarkRedColor: Code<'static> = c("111451", "Dark red color");
pub const DarkfieldIllumination: Code<'static> = c("111745", "Darkfield illumination");
pub const DataNotAccessibleFromStorage: Code<'static> = c("130752", "Data Not Accessible from Storage");
pub const DataRetentionPolicyExpired: Code<'static> = c("113039", "Data Retention Policy Expired");
pub const DateStickerIsMissing: Code<'static> = c("111193", "Date sticker is missing");
pub const DateTimeConcernNoted: Code<'static> = c("121431", "DateTime Concern Noted");
pub const DateTimeConcernResolved: Code<'static> = c("121432", "DateTime Concern Resolved");
pub const DateTimeEnded: Code<'static> = c("111527", "DateTime Ended");
pub const DateTimeEstimated: Code<'static> = c("121137", "DateTime Estimated");
pub const DateTimeOfDeath: Code<'static> = c("122165", "DateTime of Death");
pub const DateTimeOfIntervention: Code<'static> = c("122105", "DateTime of Intervention");
pub const DateTimeOfLastEvaluation: Code<'static> = c("111536", "DateTime of last evaluation");
pub const DateTimeOfProcessing: Code<'static> = c("111702", "DateTime of processing");
pub const DateTimeOfRecordingOfLogEntry: Code<'static> = c("121125", "DateTime of Recording of Log Entry");
pub const DateTimeProblemObserved: Code<'static> = c("111535", "DateTime problem observed");
pub const DateTimeProblemResolved: Code<'static> = c("121433", "DateTime Problem Resolved");
pub const DateTimeStarted: Code<'static> = c("111526", "DateTime Started");
pub const DateTimeUnsynchronized: Code<'static> = c("121136", "DateTime Unsynchronized");
pub const DeIdentifyingEquipment: Code<'static> = c("109104", "De-identifying Equipment");
pub const DeathDuringCatheterization: Code<'static> = c("122167", "Death During Catheterization");
pub const DeathDuringThisAdmission: Code<'static> = c("122166", "Death During This Admission");
pub const DeepCapillaryPlexusFlow: Code<'static> = c("128310", "Deep capillary plexus flow");
pub const DeepCapillaryPlexusMap: Code<'static> = c("128311", "Deep capillary plexus map");
pub const DeepRetinaStructuralReflectanceMap: Code<'static> = c("128270", "Deep retina structural reflectance map");
pub const DeepRetinaVasculatureFlow: Code<'static> = c("128269", "Deep retina vasculature flow");
pub const DeformableModels: Code<'static> = c("123111", "Deformable Models");
pub const DeformedForRegistration: Code<'static> = c("125027", "Deformed for Registration");
pub const DegreeOfFreedomExactRotationalValue: Code<'static> = c("112379", "Degree of Freedom Exact Rotational Value");
pub const DegreeOfFreedomExactTranslationalValue: Code<'static> = c("112376", "Degree of Freedom Exact Translational Value");
pub const DegreeOfFreedomID: Code<'static> = c("112363", "Degree of Freedom ID");
pub const DegreeOfFreedomMaximumRotationalValue: Code<'static> = c("112381", "Degree of Freedom Maximum Rotational Value");
pub const DegreeOfFreedomMaximumTranslationalValue: Code<'static> = c("112378", "Degree of Freedom Maximum Translational Value");
pub const DegreeOfFreedomMinimumRotationalValue: Code<'static> = c("112380", "Degree of Freedom Minimum Rotational Value");
pub const DegreeOfFreedomMinimumTranslationalValue: Code<'static> = c("112377", "Degree of Freedom Minimum Translational Value");
pub const DegreeOfThrombus: Code<'static> = c("122131", "Degree of Thrombus");
pub const DegreesOfFreedomSpecification: Code<'static> = c("112362", "Degrees of Freedom Specification");
pub const DelayExportUntilClinicalInformationIsAvailable: Code<'static> = c("128716", "Delay export until clinical information is available");
pub const DelayExportUntilConfirmationOfDiagnosisIsAvailable: Code<'static> = c("128717", "Delay export until confirmation of diagnosis is available");
pub const DelayExportUntilExpertReviewIsAvailable: Code<'static> = c("128722", "Delay export until expert review is available");
pub const DelayExportUntilFinalReportIsAvailable: Code<'static> = c("128715", "Delay export until final report is available");
pub const DelayExportUntilHistopathologyIsAvailable: Code<'static> = c("128718", "Delay export until histopathology is available");
pub const DelayExportUntilOtherLaboratoryResultsAreAvailable: Code<'static> = c("128719", "Delay export until other laboratory results are available");
pub const DelayExportUntilPatientDies: Code<'static> = c("128721", "Delay export until patient dies");
pub const DelayExportUntilPatientIsDischarged: Code<'static> = c("128720", "Delay export until patient is discharged");
pub const DelayedRedistributionState: Code<'static> = c("109094", "Delayed Redistribution State");
pub const DeliveryRateToleranceViolation: Code<'static> = c("130470", "Delivery Rate Tolerance Violation");
pub const DeliveryVerificationNote: Code<'static> = c("130031", "Delivery Verification Note");
pub const DensitometricLuminalCrossSectionalAreaGraph: Code<'static> = c("122517", "Densitometric Luminal Cross-sectional Area Graph");
pub const DensitometricMethod: Code<'static> = c("122474", "Densitometric Method");
pub const Density: Code<'static> = c("112118", "Density");
#[deprecated = "retired in PS3.16"]
pub const Density_111103: Code<'static> = c("111103", "Density");
pub const DepartmentSystemScheduler: Code<'static> = c("DSS", "Department System Scheduler");
pub const DependentOpacity: Code<'static> = c("112119", "Dependent opacity");
pub const Deployment: Code<'static> = c("121155", "Deployment");
pub const DeprecatedProtocol: Code<'static> = c("128610", "Deprecated protocol");
pub const Depth: Code<'static> = c("111020", "Depth");
pub const Derivation: Code<'static> = c("121401", "Derivation");
pub const DerivedDose: Code<'static> = c("131407", "Derived Dose");
pub const DerivedEffectiveDiameter: Code<'static> = c("113933", "Derived Effective Diameter");
pub const DerivedFiducial: Code<'static> = c("112357", "Derived Fiducial");
pub const DerivedFromHumanReadableReports: Code<'static> = c("113868", "Derived From Human-Readable Reports");
pub const DerivedHemodynamicMeasurements: Code<'static> = c("122126", "Derived Hemodynamic Measurements");
pub const DerivedImagingMeasurements: Code<'static> = c("126011", "Derived Imaging Measurements");
pub const DerivedNonValveArea: Code<'static> = c("122160", "Derived Non-Valve Area");
pub const DerivedPlanningData: Code<'static> = c("112373", "Derived Planning Data");
pub const DerivedPlanningImages: Code<'static> = c("112372", "Derived Planning Images");
pub const Dermoscopy: Code<'static> = c("DMS", "Dermoscopy");
pub const DescriptionOfChange: Code<'static> = c("111021", "Description of Change");
pub const DescriptionOfMaterial: Code<'static> = c("121145", "Description of Material");
pub const Designator: Code<'static> = c("129002", "Designator");
pub const DestinationMedia: Code<'static> = c("110154", "Destination Media");
pub const DestinationRoleID: Code<'static> = c("110152", "Destination Role ID");
pub const DestructionOfMicrobubbles: Code<'static> = c("125235", "Destruction of microbubbles");
pub const Detach: Code<'static> = c("110125", "Detach");
pub const DetectionPerformed: Code<'static> = c("111022", "Detection Performed");
pub const DetectorArtifactS: Code<'static> = c("111214", "Detector artifact(s)");
pub const DetectorType: Code<'static> = c("113947", "Detector Type");
pub const Deterministic: Code<'static> = c("113211", "Deterministic");
pub const DeviationIndex: Code<'static> = c("113847", "Deviation Index");
pub const Device: Code<'static> = c("121007", "Device");
pub const DeviceCode: Code<'static> = c("121150", "Device Code");
pub const DeviceCrossedLesion: Code<'static> = c("122089", "Device crossed lesion");
pub const DeviceDeployed: Code<'static> = c("122305", "Device deployed");
pub const DeviceDescription: Code<'static> = c("120999", "Device Description");
pub const DeviceLocalizer: Code<'static> = c("121341", "Device localizer");
pub const DeviceManufacturer: Code<'static> = c("113878", "Device Manufacturer");
pub const DeviceModelName: Code<'static> = c("113879", "Device Model Name");
pub const DeviceName: Code<'static> = c("113877", "Device Name");
pub const DeviceObserverManufacturer: Code<'static> = c("121014", "Device Observer Manufacturer");
pub const DeviceObserverManufacturerClassUID: Code<'static> = c("121061", "Device Observer Manufacturer Class UID");
pub const DeviceObserverModelName: Code<'static> = c("121015", "Device Observer Model Name");
pub const DeviceObserverName: Code<'static> = c("121013", "Device Observer Name");
pub const DeviceObserverPhysicalLocationDuringObservation: Code<'static> = c("121017", "Device Observer Physical Location During Observation");
pub const DeviceObserverSerialNumber: Code<'static> = c("121016", "Device Observer Serial Number");
pub const DeviceObserverUID: Code<'static> = c("121012", "Device Observer UID");
pub const DeviceRelatedAndEnvironmentRelatedEvent: Code<'static> = c("130864", "Device-related and Environment-related Event");
pub const DeviceRoleInProcedure: Code<'static> = c("113876", "Device Role in Procedure");
pub const DeviceSerialNumber: Code<'static> = c("113880", "Device Serial Number");
pub const DeviceSubject: Code<'static> = c("121192", "Device Subject");
pub const DeviceSubjectManufacturer: Code<'static> = c("121194", "Device Subject Manufacturer");
pub const DeviceSubjectModelName: Code<'static> = c("121195", "Device Subject Model Name");
pub const DeviceSubjectName: Code<'static> = c("121193", "Device Subject Name");
pub const DeviceSubjectPhysicalLocationDuringObservation: Code<'static> = c("121197", "Device Subject Physical Location during observation");
pub const DeviceSubjectSerialNumber: Code<'static> = c("121196", "Device Subject Serial Number");
pub const DeviceSubjectUID: Code<'static> = c("121198", "Device Subject UID");
pub const DfCD4589Zr: Code<'static> = c("126520", "Df-CD45 ^89^Zr");
pub const DfFK23PEG489Zr: Code<'static> = c("126763", "Df-[FK](2)-3PEG(4) ^89^Zr");
pub const DfFK289Zr: Code<'static> = c("126762", "Df-[FK](2) ^89^Zr");
pub const DfFK89Zr: Code<'static> = c("126760", "Df-FK ^89^Zr");
pub const DfFKPEG389Zr: Code<'static> = c("126761", "Df-FK-PEG(3) ^89^Zr");
pub const DgN: Code<'static> = c("128407", "DgN");
pub const DiabeticFibrousMastopathy: Code<'static> = c("111259", "Diabetic fibrous mastopathy");
pub const DiagnosisInputUsed: Code<'static> = c("128218", "Diagnosis Input Used");
pub const DiagnosticAdministration: Code<'static> = c("130249", "Diagnostic Administration");
pub const DiagnosticSourceImages: Code<'static> = c("128181", "Diagnostic Source Images");
pub const DiameterBasedMethod: Code<'static> = c("122651", "Diameter Based Method");
pub const DiameterGraph: Code<'static> = c("122509", "Diameter Graph");
pub const DiameterRangeMaximumValue: Code<'static> = c("130903", "Diameter Range Maximum Value");
pub const DiameterRangeMinimumValue: Code<'static> = c("130902", "Diameter Range Minimum Value");
pub const DiameterSymmetry: Code<'static> = c("122544", "Diameter Symmetry");
pub const DiamondDetector: Code<'static> = c("128708", "Diamond Detector");
pub const Diaphanography: Code<'static> = c("DG", "Diaphanography");
pub const DiaphragmaticAttenuation: Code<'static> = c("122745", "Diaphragmatic attenuation");
pub const DiastolicBloodVelocityMean: Code<'static> = c("122201", "Diastolic blood velocity, mean");
pub const DiastolicBloodVelocityPeak: Code<'static> = c("122202", "Diastolic blood velocity, peak");
pub const DiastolicPerSystolicVelocityRatio: Code<'static> = c("122218", "Diastolic/Systolic velocity ratio");
#[deprecated = "retired in PS3.16"]
pub const DiastolicPressureAverage: Code<'static> = c("109020", "Diastolic pressure, average");
#[deprecated = "retired in PS3.16"]
pub const DiastolicPressureNadir: Code<'static> = c("109021", "Diastolic pressure nadir");
pub const DiceSimilarityCoefficient: Code<'static> = c("130920", "Dice Similarity Coefficient");
pub const Dictation: Code<'static> = c("110011", "Dictation");
#[deprecated = "retired in PS3.16"]
pub const DifferenceAverageOfGLCM: Code<'static> = c("128784", "Difference Average of GLCM");
#[deprecated = "retired in PS3.16"]
pub const DifferenceEntropyOfGLCM: Code<'static> = c("128786", "Difference Entropy of GLCM");
#[deprecated = "retired in PS3.16"]
pub const DifferenceInBorderDefinition: Code<'static> = c("112166", "Difference in border definition");
#[deprecated = "retired in PS3.16"]
pub const DifferenceInBorderShape: Code<'static> = c("112165", "Difference in border shape");
#[deprecated = "retired in PS3.16"]
pub const DifferenceInDistribution: Code<'static> = c("112167", "Difference in distribution");
pub const DifferenceInEDLVVolume: Code<'static> = c("122769", "Difference in ED LV Volume");
pub const DifferenceInEjectionFraction: Code<'static> = c("122768", "Difference in Ejection Fraction");
#[deprecated = "retired in PS3.16"]
pub const DifferenceInSiteInvolvement: Code<'static> = c("112168", "Difference in site involvement");
#[deprecated = "retired in PS3.16"]
pub const DifferenceInTexture: Code<'static> = c("112170", "Difference in Texture");
#[deprecated = "retired in PS3.16"]
pub const DifferenceInTypeOfContent: Code<'static> = c("112169", "Difference in Type of Content");
#[deprecated = "retired in PS3.16"]
pub const DifferenceVarianceOfGLCM: Code<'static> = c("128785", "Difference Variance of GLCM");
pub const DifferentialDiagnosisPerImpression: Code<'static> = c("111023", "Differential Diagnosis/Impression");
pub const DifferentialInterferenceContrast: Code<'static> = c("111748", "Differential interference contrast");
pub const DifferentialSignal: Code<'static> = c("109006", "Differential signal");
pub const DifficultPhysicalPerClinicalExamination: Code<'static> = c("111479", "Difficult physical/clinical examination");
pub const DiffuseContrastEnhancement: Code<'static> = c("130603", "Diffuse Contrast Enhancement");
pub const DiffuseDirectIllumination: Code<'static> = c("111625", "Diffuse direct illumination");
pub const DiffuselyIncreasedVascularityInSurroundingTissue: Code<'static> = c("111377", "Diffusely increased vascularity in surrounding tissue");
pub const DiffusionAnisotropy: Code<'static> = c("113044", "Diffusion Anisotropy");
pub const DiffusionAttenuated: Code<'static> = c("113045", "Diffusion Attenuated");
pub const DiffusionCoefficient: Code<'static> = c("113289", "Diffusion Coefficient");
pub const DiffusionImageAnalysis: Code<'static> = c("129105", "Diffusion image analysis");
pub const DiffusionTractography: Code<'static> = c("129106", "Diffusion tractography");
pub const DiffusionWeighted: Code<'static> = c("113043", "Diffusion weighted");
pub const DiffusionWeightedAcquisitionHighestBValueImage: Code<'static> = c("130566", "Diffusion-weighted Acquisition Highest b-value image");
#[deprecated = "retired in PS3.16"]
pub const DigitalFluoroscopy: Code<'static> = c("DF", "Digital fluoroscopy");
#[deprecated = "retired in PS3.16"]
pub const DigitalMammography: Code<'static> = c("111409", "Digital Mammography");
#[deprecated = "retired in PS3.16"]
pub const DigitalMicroscopy: Code<'static> = c("DM", "Digital microscopy");
pub const DigitalRadiography: Code<'static> = c("DX", "Digital Radiography");
#[deprecated = "retired in PS3.16"]
pub const DigitalSubtractionAngiography: Code<'static> = c("DS", "Digital Subtraction Angiography");
pub const DigitalTimecodeNOS: Code<'static> = c("109001", "Digital timecode (NOS)");
pub const Diode: Code<'static> = c("128704", "Diode");
pub const DiodeArray: Code<'static> = c("128702", "Diode Array");
pub const DirectDetector: Code<'static> = c("113948", "Direct Detector");
pub const DirectlyMeasured: Code<'static> = c("125316", "Directly measured");
pub const DisagreementWithPriorFindings: Code<'static> = c("122776", "Disagreement with prior findings");
pub const DisapprovedForAnyUse: Code<'static> = c("128609", "Disapproved for any use");
pub const DisapprovedForExperimentalUse: Code<'static> = c("128612", "Disapproved for experimental use");
pub const DisapprovedForUseAtTheInstitution: Code<'static> = c("128623", "Disapproved for use at the institution");
pub const DisapprovedForUseInTheClinicalTrial: Code<'static> = c("128624", "Disapproved for use in the clinical trial");
pub const DisapprovedForUseOnPregnantPatients: Code<'static> = c("128617", "Disapproved for use on pregnant patients");
pub const DiscCentered: Code<'static> = c("111901", "Disc centered");
pub const DiscFovea: Code<'static> = c("111934", "Disc-Fovea");
pub const DiscMaculaCentered: Code<'static> = c("111903", "Disc-macula centered");
pub const Discarded: Code<'static> = c("111458", "Discarded");
pub const DischargeDateTime: Code<'static> = c("122163", "Discharge DateTime");
pub const DischargeSummary: Code<'static> = c("121115", "Discharge Summary");
pub const DiscontinuedForUnspecifiedReason: Code<'static> = c("110513", "Discontinued for unspecified reason");
pub const DiscontinuedProcedureStepRescheduled: Code<'static> = c("110528", "Discontinued Procedure Step rescheduled");
pub const DiscontinuedProcedureStepReschedulingRecommended: Code<'static> = c("110529", "Discontinued Procedure Step rescheduling recommended");
pub const DissectionClassification: Code<'static> = c("122387", "Dissection Classification");
pub const DissectionWithEntireSpecimenSubmission: Code<'static> = c("111726", "Dissection with entire specimen submission");
pub const DissectionWithRepresentativeSectionsSubmission: Code<'static> = c("111727", "Dissection with representative sections submission");
#[deprecated = "retired in PS3.16"]
pub const DissimilarityOfGLCM: Code<'static> = c("126064", "Dissimilarity of GLCM");
pub const DistalCentralizer: Code<'static> = c("112325", "Distal Centralizer");
pub const DistalFindingSite: Code<'static> = c("121117", "Distal Finding Site");
pub const DistalReference: Code<'static> = c("122381", "Distal Reference");
pub const DistalStentMargin: Code<'static> = c("122386", "Distal Stent Margin");
pub const Distance: Code<'static> = c("121206", "Distance");
pub const DistanceCorrection: Code<'static> = c("128527", "Distance Correction");
pub const DistanceFromAnus: Code<'static> = c("112233", "Distance from anus");
pub const DistanceFromChestWall: Code<'static> = c("121244", "Distance from chest wall");
pub const DistanceFromNeurovascularBundle: Code<'static> = c("130557", "Distance from neurovascular bundle");
pub const DistanceFromNipple: Code<'static> = c("121242", "Distance from nipple");
pub const DistanceFromSkin: Code<'static> = c("121243", "Distance from skin");
pub const DistancePupillaryDistance: Code<'static> = c("111679", "Distance Pupillary Distance");
pub const DistanceSourceToDetector: Code<'static> = c("113750", "Distance Source to Detector");
pub const DistanceSourceToIsocenter: Code<'static> = c("113748", "Distance Source to Isocenter");
pub const DistanceSourceToReferencePoint: Code<'static> = c("113737", "Distance Source to Reference Point");
pub const DistanceSourceToTablePlane: Code<'static> = c("113792", "Distance Source to Table Plane");
pub const DistinctlyDefined: Code<'static> = c("112138", "Distinctly defined");
pub const DistortedImplant: Code<'static> = c("111506", "Distorted implant");
pub const DistortionArtifactInTheAreaOfInterest: Code<'static> = c("130586", "Distortion artifact in the area of interest");
pub const DistributedDiffusionCoefficient: Code<'static> = c("113298", "Distributed Diffusion Coefficient");
pub const DistributionDescriptor: Code<'static> = c("112006", "Distribution Descriptor");
pub const DistributionRepresentation: Code<'static> = c("128413", "Distribution Representation");
pub const DivideByZero: Code<'static> = c("114003", "Divide by zero");
pub const DoctorCanceledProcedure: Code<'static> = c("110500", "Doctor canceled procedure");
pub const DocumentDigitizerEquipment: Code<'static> = c("DOCD", "Document Digitizer Equipment");
pub const DocumentTitle: Code<'static> = c("121144", "Document Title");
pub const DocumentTitleModifier: Code<'static> = c("113011", "Document Title Modifier");
pub const Document_121003: Code<'static> = c("121003", "Document");
pub const Document_DOC: Code<'static> = c("DOC", "Document");
pub const DogRegistryOfAmerica: Code<'static> = c("109212", "Dog Registry of America");
pub const DominantFollicle: Code<'static> = c("130909", "Dominant Follicle");
pub const DoorInterlock: Code<'static> = c("130478", "Door Interlock");
pub const DopplerAngle: Code<'static> = c("125106", "Doppler Angle");
pub const DopplerAudio: Code<'static> = c("109113", "Doppler audio");
pub const DopplerOCTA: Code<'static> = c("128256", "Doppler OCT-A");
pub const DopplerVolumeFlow: Code<'static> = c("125219", "Doppler Volume Flow");
pub const DoseAreaProduct: Code<'static> = c("122130", "Dose Area Product");
pub const DoseAreaProductTotal: Code<'static> = c("113722", "Dose Area Product Total");
pub const DoseCalculationBoundingVolume: Code<'static> = c("130065", "Dose Calculation Bounding Volume");
pub const DoseCalculationImageSeries: Code<'static> = c("128186", "Dose Calculation Image Series");
pub const DoseCalibrator: Code<'static> = c("113541", "Dose Calibrator");
pub const DoseCheckAlertDetails: Code<'static> = c("113900", "Dose Check Alert Details");
pub const DoseCheckNotificationDetails: Code<'static> = c("113908", "Dose Check Notification Details");
pub const DoseFrequency: Code<'static> = c("111578", "Dose frequency");
pub const DoseFromPriorTreatment: Code<'static> = c("129212", "Dose from prior Treatment");
pub const DoseGradient: Code<'static> = c("128488", "Dose Gradient");
pub const DoseImage: Code<'static> = c("121342", "Dose Image");
pub const DoseLimitInterlock: Code<'static> = c("130738", "Dose Limit Interlock");
pub const DoseMeasurementDevice: Code<'static> = c("113794", "Dose Measurement Device");
pub const DosePointCloud: Code<'static> = c("128496", "Dose Point Cloud");
pub const DoseRP: Code<'static> = c("113738", "Dose (RP)");
pub const DoseRPTotal: Code<'static> = c("113725", "Dose (RP) Total");
pub const DoseRate: Code<'static> = c("131401", "Dose Rate");
pub const DoseRateToleranceViolation: Code<'static> = c("130474", "Dose Rate Tolerance Violation");
pub const DosimeterType: Code<'static> = c("113818", "Dosimeter Type");
pub const DosimetricResult: Code<'static> = c("128190", "Dosimetric Result");
pub const DosingFactor: Code<'static> = c("130190", "Dosing Factor");
pub const DoubleExposure: Code<'static> = c("113026", "Double exposure");
pub const DoubleMDerivedFilter: Code<'static> = c("130767", "Double-m-derived filter");
pub const DoubleProduct: Code<'static> = c("122708", "Double Product");
pub const DrugAdministered: Code<'static> = c("122083", "Drug administered");
pub const DrugEnd: Code<'static> = c("122082", "Drug end");
pub const DrugProductIdentifier: Code<'static> = c("113510", "Drug Product Identifier");
pub const DrugStart: Code<'static> = c("122081", "Drug start");
pub const DualSideIllumination: Code<'static> = c("130811", "Dual-side illumination");
pub const DualSpeedOfSoundCorrection: Code<'static> = c("130819", "Dual Speed of Sound Correction");
pub const DuctNarrowing: Code<'static> = c("111429", "Duct narrowing");
pub const DuctalAdenoma: Code<'static> = c("111258", "Ductal adenoma");
#[deprecated = "retired in PS3.16"]
pub const DuctalHyperplasia: Code<'static> = c("111249", "Ductal hyperplasia");
#[deprecated = "retired in PS3.16"]
pub const Ductography: Code<'static> = c("111139", "Ductography");
pub const DuctusArteriosusArch: Code<'static> = c("131021", "Ductus Arteriosus Arch");
#[deprecated = "retired in PS3.16"]
pub const DuplexDoppler: Code<'static> = c("DD", "Duplex Doppler");
pub const DuplicateOrder: Code<'static> = c("110510", "Duplicate order");
pub const DurationOfAdministration: Code<'static> = c("122095", "Duration of administration");
pub const DurationOfIntervention: Code<'static> = c("122106", "Duration of Intervention");
pub const DurationOfTimePeriod: Code<'static> = c("130532", "Duration of Time Period");
pub const DuringTransport: Code<'static> = c("127102", "During transport");
pub const DuringVoiding: Code<'static> = c("109137", "During voiding");
pub const DwConversionFactorCoefficients: Code<'static> = c("113991", "Dw Conversion Factor Coefficients");
pub const DynamicContrastEnhancedAcquisition: Code<'static> = c("130567", "Dynamic Contrast-Enhanced Acquisition");
pub const DynamicContrastEnhancedAcquisitionSubtractionImage: Code<'static> = c("130568", "Dynamic Contrast-Enhanced Acquisition Subtraction image");
pub const DynamicContrastEnhancedTemporalResolution: Code<'static> = c("130547", "Dynamic contrast-enhanced temporal resolution");
pub const DynamicContrastMRMeasurementReport: Code<'static> = c("126002", "Dynamic Contrast MR Measurement Report");
pub const DynamicLight: Code<'static> = c("111630", "Dynamic light");
pub const DynamicWedge: Code<'static> = c("130348", "Dynamic Wedge");
pub const DyssynchronyIndex: Code<'static> = c("125325", "Dyssynchrony Index");
pub const E4G1089Zr: Code<'static> = c("126519", "E4G10 ^89^Zr");
pub const ECGAnnotation: Code<'static> = c("130866", "ECG Annotation");
pub const ECGBasedGatingSignalProcessed: Code<'static> = c("109002", "ECG-based gating signal, processed");
pub const ECGBasedModulation: Code<'static> = c("114103", "ECG-based modulation");
pub const ECGGlobalMeasurements: Code<'static> = c("122158", "ECG Global Measurements");
pub const ECGLeadMeasurements: Code<'static> = c("122159", "ECG Lead Measurements");
pub const ECST: Code<'static> = c("122656", "ECST");
pub const EEGAnnotation: Code<'static> = c("130861", "EEG Annotation");
pub const EEMCrossSectionalArea: Code<'static> = c("122333", "EEM Cross-Sectional Area");
pub const EEMDiameter: Code<'static> = c("122330", "EEM Diameter");
pub const EEMDiameterRatio: Code<'static> = c("122352", "EEM Diameter Ratio");
pub const EEMVolume: Code<'static> = c("122371", "EEM Volume");
pub const EMGAnnotation: Code<'static> = c("130862", "EMG Annotation");
pub const EOGAnnotation: Code<'static> = c("130863", "EOG Annotation");
pub const EPProcedurePhase: Code<'static> = c("109061", "EP Procedure Phase");
pub const ESUR2012ProstateMRIAcquisitionRequirements: Code<'static> = c("130606", "ESUR 2012 prostate MRI acquisition requirements");
pub const EVO: Code<'static> = c("111873", "EVO");
pub const EarlyContrastEnhancement: Code<'static> = c("130602", "Early Contrast Enhancement");
pub const EarlyGestation: Code<'static> = c("125009", "Early Gestation");
pub const EchoPattern: Code<'static> = c("111360", "Echo pattern");
#[deprecated = "retired in PS3.16"]
pub const Echocardiography: Code<'static> = c("EC", "Echocardiography");
pub const EchogenicHalo: Code<'static> = c("111359", "Echogenic halo");
pub const Echogenicity: Code<'static> = c("110849", "Echogenicity");
pub const Echovist: Code<'static> = c("125902", "Echovist");
pub const Ecromeximab89Zr: Code<'static> = c("126732", "Ecromeximab ^89^Zr");
pub const EdgeDetection: Code<'static> = c("123103", "Edge Detection");
pub const EdgeEnhancement: Code<'static> = c("113086", "Edge enhancement");
pub const EditedModel: Code<'static> = c("129010", "Edited Model");
pub const EducationalIntent: Code<'static> = c("129012", "Educational Intent");
pub const EffectiveAtomicNumber: Code<'static> = c("129320", "Effective Atomic Number");
pub const EffectiveDose: Code<'static> = c("113839", "Effective Dose");
pub const EffectiveDoseConversionFactor: Code<'static> = c("113840", "Effective Dose Conversion Factor");
pub const EffectiveDosePhantomType: Code<'static> = c("113817", "Effective Dose Phantom Type");
pub const EffectiveOrificeArea: Code<'static> = c("125326", "Effective Orifice Area");
pub const EffectiveZ: Code<'static> = c("130084", "Effective Z");
pub const EffectiveZPerA: Code<'static> = c("130085", "Effective Z per A");
pub const Elasticity: Code<'static> = c("110830", "Elasticity");
pub const ElectricalFailure: Code<'static> = c("111217", "Electrical failure");
pub const Electrocardiography: Code<'static> = c("ECG", "Electrocardiography");
pub const ElectrodePlacement: Code<'static> = c("122141", "Electrode Placement");
pub const Electroencephalography: Code<'static> = c("EEG", "Electroencephalography");
pub const ElectromechanicalDelay: Code<'static> = c("125320", "Electromechanical Delay");
pub const Electromyography: Code<'static> = c("EMG", "Electromyography");
pub const ElectronFixedAperture: Code<'static> = c("130343", "Electron Fixed Aperture");
pub const Electrooculography: Code<'static> = c("EOG", "Electrooculography");
pub const ElementalCompositionAtomicMassFraction: Code<'static> = c("130094", "Elemental Composition Atomic Mass Fraction");
pub const ElevationBasedCornealTomographer: Code<'static> = c("111945", "Elevation-based corneal tomographer");
pub const Eligibility: Code<'static> = c("126075", "Eligibility");
pub const EligibilityReader: Code<'static> = c("129001", "Eligibility Reader");
pub const EligibleForReimbursement: Code<'static> = c("128613", "Eligible for reimbursement");
pub const EligibleForReimbursementOnPerPatientBasis: Code<'static> = c("128614", "Eligible for reimbursement on per patient basis");
pub const Elliptic: Code<'static> = c("112134", "Elliptic");
pub const EllipticFilter: Code<'static> = c("130762", "Elliptic filter");
pub const EllipticalAnnulusSectors: Code<'static> = c("131304", "Elliptical annulus sectors");
pub const Email: Code<'static> = c("110031", "Email");
pub const EmailMessage: Code<'static> = c("110157", "Email Message");
pub const EmbryonicKidney: Code<'static> = c("127801", "Embryonic Kidney");
pub const EmergencyOverrideStarted: Code<'static> = c("110127", "Emergency Override Started");
pub const EmergencyOverrideStopped: Code<'static> = c("110138", "Emergency Override Stopped");
pub const EmpiricalAlgorithm: Code<'static> = c("128481", "Empirical Algorithm");
pub const EndCirculatorySupport: Code<'static> = c("121158", "End Circulatory Support");
#[deprecated = "retired in PS3.16"]
pub const EndDiastole: Code<'static> = c("109022", "End diastole");
pub const EndOfExpiration: Code<'static> = c("109023", "End of expiration");
pub const EndOfInspiration: Code<'static> = c("109024", "End of inspiration");
#[deprecated = "retired in PS3.16"]
pub const EndOfSystole: Code<'static> = c("109070", "End of systole");
pub const EndOfTimePeriod: Code<'static> = c("130534", "End of Time Period");
pub const EndOfXRayIrradiation: Code<'static> = c("113810", "End of X-Ray Irradiation");
pub const EndOxygenAdministration: Code<'static> = c("121162", "End oxygen administration");
pub const EndPacing: Code<'static> = c("121167", "End Pacing");
pub const EndProcedureAction: Code<'static> = c("121131", "End Procedure Action");
pub const EndVentilation: Code<'static> = c("121169", "End Ventilation");
pub const EndingFlowRateOfAdministration: Code<'static> = c("130209", "Ending Flow Rate of administration");
pub const EndocardialMethod: Code<'static> = c("130683", "Endocardial Method");
pub const EndocavitaryTransducer: Code<'static> = c("125266", "Endocavitary Transducer");
pub const Endoleak: Code<'static> = c("122680", "endoleak");
pub const EndorectalCoilType: Code<'static> = c("130544", "Endorectal coil type");
pub const EndorectalCoilUsed: Code<'static> = c("130543", "Endorectal coil used");
pub const EndorectalTransducer: Code<'static> = c("125264", "Endorectal Transducer");
pub const Endoscopy: Code<'static> = c("ES", "Endoscopy");
pub const EndothelialCellCountKeyMeasurements: Code<'static> = c("131245", "Endothelial Cell Count Key Measurements");
pub const EndothelialCellDensity: Code<'static> = c("131309", "Endothelial cell density");
pub const EndovaginalTransducer: Code<'static> = c("125263", "Endovaginal Transducer");
pub const EnhancedMultiFrameConversionEquipment: Code<'static> = c("109106", "Enhanced Multi-frame Conversion Equipment");
pub const EnrichmentManufacturer: Code<'static> = c("127191", "Enrichment manufacturer");
pub const EnrichmentMaterial: Code<'static> = c("127190", "Enrichment material");
pub const EnrichmentMaterialPresent: Code<'static> = c("127192", "Enrichment material present");
pub const EntireBodyTargetVolume: Code<'static> = c("130055", "Entire Body Target Volume");
pub const EntirePullback: Code<'static> = c("122383", "Entire Pullback");
pub const EntranceExposureAtRP: Code<'static> = c("111636", "Entrance Exposure at RP");
pub const EnvironmentalExposureHistory: Code<'static> = c("111547", "Environmental Exposure History");
pub const EnvironmentalFactor: Code<'static> = c("111548", "Environmental Factor");
pub const EpicardialMethod: Code<'static> = c("130684", "Epicardial Method");
#[deprecated = "retired in PS3.16"]
pub const EpidermalInclusionCyst: Code<'static> = c("111262", "Epidermal inclusion cyst");
pub const EpifluorescenceIllumination: Code<'static> = c("111743", "Epifluorescence illumination");
pub const Equation: Code<'static> = c("121420", "Equation");
pub const EquationCitation: Code<'static> = c("121421", "Equation Citation");
pub const EquidistantMethod: Code<'static> = c("122574", "Equidistant method");
pub const EquipmentBroughtToProcedureRoom: Code<'static> = c("122047", "Equipment brought to procedure room");
pub const EquipmentChange: Code<'static> = c("110516", "Equipment change");
pub const EquipmentFailure: Code<'static> = c("110501", "Equipment failure");
#[deprecated = "retired in PS3.16"]
pub const EquipmentFailure_122046: Code<'static> = c("122046", "Equipment failure");
pub const EquipmentIdentification: Code<'static> = c("121122", "Equipment Identification");
pub const EquipmentLandmark: Code<'static> = c("128750", "Equipment Landmark");
pub const EquipmentLandmarkToPatientFiducialZDistance: Code<'static> = c("128756", "Equipment Landmark to Patient Fiducial Z Distance");
pub const EquipmentLandmarkXPosition: Code<'static> = c("128752", "Equipment Landmark X Position");
pub const EquipmentLandmarkZPosition: Code<'static> = c("128753", "Equipment Landmark Z Position");
pub const EquipmentOrigin: Code<'static> = c("130537", "Equipment Origin");
pub const EquipmentReady: Code<'static> = c("122048", "Equipment ready");
pub const EquipmentRemoved: Code<'static> = c("122049", "Equipment removed");
pub const Equivalent2GrayFractionsModel: Code<'static> = c("130130", "Equivalent 2-Gray Fractions Model");
pub const EquivalentAttenuatorMaterial: Code<'static> = c("128465", "Equivalent Attenuator Material");
pub const EquivalentAttenuatorThickness: Code<'static> = c("128469", "Equivalent Attenuator Thickness");
pub const EquivalentCDADocument: Code<'static> = c("121331", "Equivalent CDA Document");
pub const EquivalentDose: Code<'static> = c("128512", "Equivalent Dose");
pub const EquivalentKReading: Code<'static> = c("111756", "Equivalent K-reading");
pub const EquivalentMeaningOfConceptName: Code<'static> = c("121050", "Equivalent Meaning of Concept Name");
pub const EquivalentMeaningOfValue: Code<'static> = c("121051", "Equivalent Meaning of Value");
pub const ErgometerPower: Code<'static> = c("122704", "Ergometer power");
pub const ErodedPlaque: Code<'static> = c("122390", "Eroded Plaque");
pub const EsophagealPressure: Code<'static> = c("130425", "Esophageal Pressure");
#[deprecated = "retired in PS3.16"]
pub const Estimated: Code<'static> = c("121427", "Estimated");
pub const EstimatedExtravasationActivity: Code<'static> = c("113506", "Estimated Extravasation Activity");
pub const EstimatedExtravasationVolume: Code<'static> = c("130214", "Estimated Extravasation Volume");
pub const EstimatedFromWaterEquivalentDiameter: Code<'static> = c("113988", "Estimated from Water Equivalent Diameter");
pub const EstimatedNormalFlow: Code<'static> = c("122555", "Estimated Normal Flow");
pub const EstimatedTimeframe: Code<'static> = c("111395", "Estimated Timeframe");
pub const EstrogenReceptor: Code<'static> = c("111475", "Estrogen receptor");
pub const Euler: Code<'static> = c("113218", "Euler");
#[deprecated = "retired in PS3.16"]
pub const EuropiumOrEuropiumCompound: Code<'static> = c("113711", "Europium or Europium compound");
pub const EventButtonPressed: Code<'static> = c("130893", "Event button pressed");
pub const EventButtonTest: Code<'static> = c("130894", "Event button test");
pub const EventUIDUsed: Code<'static> = c("128429", "Event UID Used");
pub const ExcellentImageQuality: Code<'static> = c("122740", "Excellent image quality");
pub const ExcisedTissue: Code<'static> = c("131396", "Excised Tissue");
pub const ExcursionDistance: Code<'static> = c("125327", "Excursion Distance");
pub const ExerciseDurationAfterStressAgentInjection: Code<'static> = c("122711", "Exercise duration after stress agent injection");
pub const ExerciserDevicePresent: Code<'static> = c("127193", "Exerciser device present");
pub const ExogenousSubstance: Code<'static> = c("127400", "Exogenous substance");
pub const ExpectedADCMapIsMissing: Code<'static> = c("130585", "Expected ADC map is missing");
pub const ExpectedBValueIsMissing: Code<'static> = c("130583", "Expected b-value is missing");
pub const Explantation: Code<'static> = c("111510", "Explantation");
pub const Export: Code<'static> = c("110106", "Export");
pub const ExposedRange: Code<'static> = c("113899", "Exposed Range");
pub const Exposure: Code<'static> = c("113736", "Exposure");
pub const ExposureIndex: Code<'static> = c("113845", "Exposure Index");
pub const ExposureTime: Code<'static> = c("113824", "Exposure Time");
pub const ExposureTimePerRotation: Code<'static> = c("113834", "Exposure Time per Rotation");
#[deprecated = "retired in PS3.16"]
pub const ExposureTime_113735: Code<'static> = c("113735", "Exposure Time");
pub const ExtendedPatientAnatomyModel: Code<'static> = c("130068", "Extended Patient Anatomy Model");
pub const ExtendedRenderingForPresentation: Code<'static> = c("121334", "Extended Rendering for Presentation");
pub const ExtendedToftsModel: Code<'static> = c("126341", "Extended Tofts Model");
pub const ExternalBodyModel: Code<'static> = c("130047", "External Body Model");
pub const ExternalCameraPhotography: Code<'static> = c("XC", "External-camera Photography");
pub const ExternalDataSource: Code<'static> = c("111781", "External Data Source");
pub const ExternalTransducer: Code<'static> = c("125261", "External Transducer");
#[deprecated = "retired in PS3.16"]
pub const ExtraAbdominalDesmoid: Code<'static> = c("111260", "Extra abdominal desmoid");
pub const ExtraProstaticFinding: Code<'static> = c("130559", "Extra-prostatic Finding");
pub const ExtractionOfIndividualSubjectFromGroup: Code<'static> = c("113131", "Extraction of individual subject from group");
pub const Extravasation: Code<'static> = c("111428", "Extravasation");
pub const ExtravasationVisibleInImage: Code<'static> = c("113568", "Extravasation visible in image");
pub const ExtremelySmall: Code<'static> = c("112131", "Extremely small");
pub const FACT: Code<'static> = c("113214", "FACT");
pub const FIRFilter: Code<'static> = c("130770", "FIR filter");
pub const FLB457C11: Code<'static> = c("126706", "FLB 457 C^11^");
pub const FWPByGACampbell1991: Code<'static> = c("128040", "FWP by GA, Campbell, 1991");
pub const FWPByGAHadlock1991: Code<'static> = c("128041", "FWP by GA, Hadlock, 1991");
pub const Failed: Code<'static> = c("111224", "Failed");
pub const FailedAnalyses: Code<'static> = c("111024", "Failed Analyses");
pub const FailedDetections: Code<'static> = c("111025", "Failed Detections");
pub const FallyprideC11: Code<'static> = c("126704", "Fallypride C^11^");
pub const FallyprideF18: Code<'static> = c("126705", "Fallypride F^18^");
pub const FalseLumen: Code<'static> = c("122361", "False Lumen");
pub const FalseMarkersPerCase: Code<'static> = c("111087", "False Markers per Case");
pub const FalseMarkersPerImage: Code<'static> = c("111086", "False Markers per Image");
pub const FalsePositiveDefectFinding: Code<'static> = c("122748", "False positive defect finding");
pub const FamilyHistoryFetalMalformationPerSyndrome: Code<'static> = c("111574", "Family history, fetal malformation/syndrome");
pub const FamilyHistoryOfMelanomaInSitu: Code<'static> = c("130481", "Family history of melanoma in situ");
pub const FamilyHistoryOfNonMelanomaSkinCancer: Code<'static> = c("130480", "Family history of non-melanoma skin cancer");
pub const FamilyHistoryOfProstateCancer: Code<'static> = c("111562", "Family history of prostate cancer");
pub const FamilyHistoryUnknown: Code<'static> = c("111563", "Family history unknown");
pub const FamilyMemberWithRiskFactor: Code<'static> = c("111537", "Family Member with Risk Factor");
pub const FastDiffusionCoefficient: Code<'static> = c("113292", "Fast Diffusion Coefficient");
pub const FastDiffusionCoefficientFraction: Code<'static> = c("113293", "Fast Diffusion Coefficient Fraction");
pub const FastMode: Code<'static> = c("114211", "Fast mode");
pub const FastingDuration: Code<'static> = c("113550", "Fasting Duration");
pub const FatFraction: Code<'static> = c("129100", "Fat fraction");
pub const FeatureDetectedOnImagesFromMultipleModalities: Code<'static> = c("111159", "Feature detected on images from multiple modalities");
pub const FeatureDetectedOnMultipleImages: Code<'static> = c("111158", "Feature detected on multiple images");
pub const FeatureDetectedOnOnlyOneOfTheImages: Code<'static> = c("111157", "Feature detected on only one of the images");
pub const FeatureDetectedOnTheOnlyImage: Code<'static> = c("111156", "Feature detected on the only image");
pub const FederationOfInternationalCanines: Code<'static> = c("109213", "Federation of International Canines");
pub const FeedManufacturer: Code<'static> = c("127200", "Feed manufacturer");
pub const FeedProductCode: Code<'static> = c("127202", "Feed product code");
pub const FeedProductName: Code<'static> = c("127201", "Feed product name");
pub const FeedSource: Code<'static> = c("127205", "Feed source");
pub const FeedbackTemperatureRegulation: Code<'static> = c("127210", "Feedback temperature regulation");
pub const Fellow: Code<'static> = c("121088", "Fellow");
pub const Female: Code<'static> = c("F", "Female");
pub const FemaleChangedToMale: Code<'static> = c("FC", "Female changed to Male");
pub const FemalePseudohermaphrodite: Code<'static> = c("FP", "Female Pseudohermaphrodite");
pub const FemaleTypical: Code<'static> = c("131230", "Female-typical");
pub const FemoralHeadBallComponent: Code<'static> = c("112308", "Femoral Head Ball Component");
pub const FemoralHeadConeTaperComponent: Code<'static> = c("112309", "Femoral Head Cone Taper Component");
pub const FemoralHeadResurfacingComponent: Code<'static> = c("112317", "Femoral Head Resurfacing Component");
pub const FemoralStem: Code<'static> = c("112310", "Femoral Stem");
pub const FemoralStemComponent: Code<'static> = c("112313", "Femoral Stem Component");
pub const FemoralStemDistalComponent: Code<'static> = c("112311", "Femoral Stem Distal Component");
pub const FemoralStemProximalComponent: Code<'static> = c("112312", "Femoral Stem Proximal Component");
pub const FemurHeadCenterOfRotation: Code<'static> = c("112304", "Femur Head Center of Rotation");
pub const FetalAnatomySurvey: Code<'static> = c("131370", "Fetal Anatomy Survey");
pub const FetalBiometry: Code<'static> = c("125002", "Fetal Biometry");
pub const FetalBiometryRatios: Code<'static> = c("125001", "Fetal Biometry Ratios");
pub const FetalCardiacUltrasoundReport: Code<'static> = c("125196", "Fetal Cardiac Ultrasound Report");
pub const FetalCardiovascularProfile: Code<'static> = c("131030", "Fetal Cardiovascular Profile");
pub const FetalCardiovascularProfileScore: Code<'static> = c("131036", "Fetal Cardiovascular Profile Score");
pub const FetalCranium: Code<'static> = c("125004", "Fetal Cranium");
pub const FetalLongBones: Code<'static> = c("125003", "Fetal Long Bones");
pub const FetalMeasurements: Code<'static> = c("125016", "Fetal Measurements");
pub const Fetus: Code<'static> = c("121026", "Fetus");
pub const FetusCharacteristics: Code<'static> = c("125015", "Fetus Characteristics");
#[deprecated = "retired in PS3.16"]
pub const FetusNumber: Code<'static> = c("121037", "Fetus number");
pub const FetusSummary: Code<'static> = c("125008", "Fetus Summary");
pub const FibroLipidicPlaque: Code<'static> = c("122394", "Fibro-Lipidic Plaque");
pub const Fibroadenolipoma: Code<'static> = c("111264", "Fibroadenolipoma");
pub const FibroadenomatoidHyperplasia: Code<'static> = c("111263", "Fibroadenomatoid hyperplasia");
pub const Fibrocalcific: Code<'static> = c("112163", "Fibrocalcific");
pub const Fibronodular: Code<'static> = c("112148", "Fibronodular");
pub const FiducialAlignment: Code<'static> = c("125022", "Fiducial Alignment");
pub const FiducialFeature: Code<'static> = c("122340", "Fiducial feature");
pub const FiducialIntent: Code<'static> = c("112369", "Fiducial Intent");
pub const FiducialMark: Code<'static> = c("112171", "Fiducial mark");
pub const Field1ForJoslin3Field: Code<'static> = c("111621", "Field 1 for Joslin 3 field");
pub const Field2ForJoslin3Field: Code<'static> = c("111622", "Field 2 for Joslin 3 field");
pub const Field3ForJoslin3Field: Code<'static> = c("111623", "Field 3 for Joslin 3 field");
pub const FieldMapMRSignalIntensity: Code<'static> = c("110807", "Field Map MR Signal Intensity");
pub const FillerNumber: Code<'static> = c("121021", "Filler Number");
pub const FilmDigitizer: Code<'static> = c("FILMD", "Film Digitizer");
pub const FilmHolder: Code<'static> = c("130352", "Film Holder");
pub const FilmScreenMammography: Code<'static> = c("111408", "Film Screen Mammography");
pub const Film_110010: Code<'static> = c("110010", "Film");
pub const Film_113951: Code<'static> = c("113951", "Film");
pub const FilteredBackProjection: Code<'static> = c("113962", "Filtered Back Projection");
pub const Filtration: Code<'static> = c("130512", "Filtration");
pub const FinalReport: Code<'static> = c("128005", "Final Report");
pub const Finding: Code<'static> = c("121071", "Finding");
pub const FindingObservationType: Code<'static> = c("125305", "Finding Observation Type");
#[deprecated = "retired in PS3.16"]
pub const Findings: Code<'static> = c("121070", "Findings");
pub const FinePleomorphicCalcification: Code<'static> = c("111344", "Fine pleomorphic calcification");
pub const FirmSkinLesion: Code<'static> = c("130485", "Firm skin lesion");
pub const FirstChordOfAbnormalRegion: Code<'static> = c("122452", "First Chord of Abnormal Region");
pub const FirstMammogramEver: Code<'static> = c("111404", "First mammogram ever");
#[deprecated = "retired in PS3.16"]
pub const FirstMeasureOfInformationCorrelationOfGLCM: Code<'static> = c("128798", "First Measure of Information Correlation of GLCM");
pub const FirstPassLeakageProfileModel: Code<'static> = c("126343", "First Pass Leakage Profile Model");
pub const FixationDeviceAngle: Code<'static> = c("130658", "Fixation Device Angle");
pub const FixationFalseNegativePercent: Code<'static> = c("131253", "Fixation false negative percent");
pub const FixationFalseNegativeRatio: Code<'static> = c("131252", "Fixation false negative ratio");
pub const FixationFalsePositivePercent: Code<'static> = c("131251", "Fixation false positive percent");
pub const FixationFalsePositiveRatio: Code<'static> = c("131250", "Fixation false positive ratio");
pub const FixationLossesRatio: Code<'static> = c("131254", "Fixation losses ratio");
pub const FixationOrPositioningDevice: Code<'static> = c("130044", "Fixation or Positioning Device");
pub const FixedBeamDirection: Code<'static> = c("125257", "Fixed beam direction");
pub const FixedDp: Code<'static> = c("113267", "Fixed-Dp");
pub const FixedDurationPauseEnded: Code<'static> = c("130155", "Fixed duration pause ended");
pub const FixedGrid: Code<'static> = c("111641", "Fixed grid");
pub const FixedLaserSetupPoint: Code<'static> = c("130360", "Fixed Laser Setup Point");
pub const FlashDoesntIncludeCassettePerScreenPerDetectorIdentification: Code<'static> = c("111191", "Flash doesn't include cassette/screen/detector identification");
pub const FlashDoesntIncludeDateOfExamination: Code<'static> = c("111188", "Flash doesn't include date of examination");
pub const FlashDoesntIncludeFacilityNameAndLocation: Code<'static> = c("111189", "Flash doesn't include facility name and location");
pub const FlashDoesntIncludeMammographyUnitIdentification: Code<'static> = c("111192", "Flash doesn't include mammography unit identification");
pub const FlashDoesntIncludePatientNameAndAdditionalPatientId: Code<'static> = c("111187", "Flash doesn't include patient name and additional patient id");
pub const FlashDoesntIncludeTechnologistIdentification: Code<'static> = c("111190", "Flash doesn't include technologist identification");
pub const FlashIsIllegibleDoesNotFitOrIsLopsided: Code<'static> = c("111186", "Flash is illegible, does not fit, or is lopsided");
pub const FlashIsNotNearEdgeOfFilm: Code<'static> = c("111185", "Flash is not near edge of film");
pub const FlatFilter: Code<'static> = c("113653", "Flat filter");
pub const FlatteningFilterBeam: Code<'static> = c("130355", "Flattening Filter Beam");
pub const FleischTable: Code<'static> = c("122254", "Fleisch table");
pub const FleischnersLineS: Code<'static> = c("112107", "Fleischner's line(s)");
pub const Flocculent: Code<'static> = c("112164", "Flocculent");
pub const FlorbetabenF18: Code<'static> = c("126501", "Florbetaben F^18^");
pub const FlowImageForImageProcessing: Code<'static> = c("128251", "Flow image for image processing");
pub const FlowQuantification: Code<'static> = c("122604", "Flow Quantification");
pub const FlowRateAboveAdjustmentLimit: Code<'static> = c("130153", "Flow rate above adjustment limit");
pub const FlowRateAboveWarningLimit: Code<'static> = c("130152", "Flow rate above warning limit");
pub const FlowRateVsTime: Code<'static> = c("130229", "Flow Rate vs Time");
pub const FlowVariance: Code<'static> = c("110829", "Flow Variance");
pub const FlowVelocity: Code<'static> = c("110828", "Flow Velocity");
pub const FlubatineF18_126503: Code<'static> = c("126503", "Flubatine F^18^");
pub const FlubatineF18_126712: Code<'static> = c("126712", "Flubatine F^18^");
pub const Fluffy: Code<'static> = c("112149", "Fluffy");
pub const FluidObtained: Code<'static> = c("111449", "Fluid obtained");
#[deprecated = "retired in PS3.16"]
pub const FluoresceinAngiography: Code<'static> = c("FA", "Fluorescein angiography");
pub const FluoroDoseAreaProductTotal: Code<'static> = c("113726", "Fluoro Dose Area Product Total");
pub const FluoroDoseRPTotal: Code<'static> = c("113728", "Fluoro Dose (RP) Total");
pub const FluoroMode: Code<'static> = c("113732", "Fluoro Mode");
pub const FluoromisonidazoleFMISOF18: Code<'static> = c("126708", "Fluoromisonidazole (FMISO) F^18^");
pub const FluoroscopyGuidedProjectionRadiographySystem: Code<'static> = c("113957", "Fluoroscopy-Guided Projection Radiography System");
pub const FluorotrioprideF18: Code<'static> = c("126707", "Fluorotriopride F^18^");
pub const FlurpiridazF18: Code<'static> = c("126718", "Flurpiridaz F^18^");
pub const FlushAdministration: Code<'static> = c("130251", "Flush Administration");
pub const FocalAbnormality: Code<'static> = c("130591", "Focal abnormality");
pub const FocalContrastEnhancement: Code<'static> = c("130604", "Focal Contrast Enhancement");
pub const FocalPlaneZOffset: Code<'static> = c("112708", "Focal plane Z offset");
pub const FocalSpotSize: Code<'static> = c("113766", "Focal Spot Size");
pub const FocusedGrid: Code<'static> = c("111642", "Focused grid");
pub const FoldedTissue: Code<'static> = c("131506", "Folded tissue");
pub const FollicleCountGroup: Code<'static> = c("130901", "Follicle Count Group");
pub const FollicleCountGroupIdentifier: Code<'static> = c("130914", "Follicle Count Group Identifier");
pub const FollicleType: Code<'static> = c("130908", "Follicle Type");
pub const FollowUpAtShortInterval111Months: Code<'static> = c("111142", "Follow-up at short interval (1-11 months)");
pub const FollowUpAtShortIntervalFromPriorStudy: Code<'static> = c("111416", "Follow-up at short interval from prior study");
pub const FollowUpPostBiopsyAsDirectedByClinician: Code<'static> = c("111121", "Follow-up post biopsy as directed by clinician");
pub const FoodTreat: Code<'static> = c("127391", "Food treat");
pub const FootToHead: Code<'static> = c("110869", "Foot To Head");
pub const ForAttenuationCorrection: Code<'static> = c("122403", "For Attenuation Correction");
pub const ForClinicalTrialExport: Code<'static> = c("128711", "For Clinical Trial Export");
pub const ForClinicalTrialSubmission: Code<'static> = c("128207", "For Clinical Trial Submission");
pub const ForConference: Code<'static> = c("113005", "For Conference");
pub const ForDiagnosis: Code<'static> = c("128195", "For Diagnosis");
pub const ForEvaluation: Code<'static> = c("131404", "For evaluation");
pub const ForLitigation: Code<'static> = c("113021", "For Litigation");
pub const ForMachineQualityAssurance: Code<'static> = c("128205", "For Machine Quality Assurance");
pub const ForPatient: Code<'static> = c("113007", "For Patient");
pub const ForPatientSetupVerification: Code<'static> = c("128206", "For Patient Setup Verification");
pub const ForPeerReview: Code<'static> = c("113008", "For Peer Review");
pub const ForPhysicianReview: Code<'static> = c("128201", "For Physician Review");
pub const ForPhysicistReview: Code<'static> = c("128202", "For Physicist Review");
pub const ForPlanComparison: Code<'static> = c("128199", "For Plan Comparison");
pub const ForPlanQualityAssurance: Code<'static> = c("128204", "For Plan Quality Assurance");
pub const ForPrinting: Code<'static> = c("113018", "For Printing");
pub const ForProcessingPredecessor: Code<'static> = c("121358", "For Processing predecessor");
pub const ForPublicationExport: Code<'static> = c("128714", "For Publication Export");
pub const ForRTPlanSummation: Code<'static> = c("128200", "For RT Plan Summation");
pub const ForRTPrescription: Code<'static> = c("128197", "For RT Prescription");
pub const ForRTTreatmentPlanning: Code<'static> = c("128198", "For RT Treatment Planning");
pub const ForRTWorkflow: Code<'static> = c("128180", "For RT Workflow");
pub const ForReferringProvider: Code<'static> = c("113002", "For Referring Provider");
pub const ForReportAttachment: Code<'static> = c("113020", "For Report Attachment");
pub const ForResearch: Code<'static> = c("113009", "For Research");
pub const ForResearchCollectionExport: Code<'static> = c("128713", "For Research Collection Export");
pub const ForSegmentation: Code<'static> = c("128196", "For Segmentation");
pub const ForSurgery: Code<'static> = c("113003", "For Surgery");
pub const ForTeaching: Code<'static> = c("113004", "For Teaching");
pub const ForTeachingFileExport: Code<'static> = c("128710", "For Teaching File Export");
pub const ForTherapy: Code<'static> = c("113006", "For Therapy");
pub const ForTreatmentDelivery: Code<'static> = c("131403", "For treatment delivery");
pub const ForTumorBoard: Code<'static> = c("128203", "For Tumor Board");
pub const ForTumorRegistry: Code<'static> = c("128208", "For Tumor Registry");
pub const ForVerification: Code<'static> = c("131405", "For verification");
pub const ForcedAirHeater: Code<'static> = c("127250", "Forced air heater");
#[deprecated = "retired in PS3.16"]
pub const ForeignBodyReaction: Code<'static> = c("111265", "Foreign body (reaction)");
pub const ForeignMaterial: Code<'static> = c("131514", "Foreign material");
pub const FourChamberView: Code<'static> = c("131029", "Four chamber view");
pub const FovealSensitivityRelated: Code<'static> = c("111841", "Foveal sensitivity related");
pub const Fp10: Code<'static> = c("130711", "Fp10");
pub const Fp9: Code<'static> = c("130710", "Fp9");
pub const FractalDimension: Code<'static> = c("126050", "Fractal Dimension");
pub const FractionalAnisotropy: Code<'static> = c("110808", "Fractional Anisotropy");
pub const FractionalChange: Code<'static> = c("125314", "Fractional Change");
pub const FractionalKurtosisAnisotropy: Code<'static> = c("113209", "Fractional Kurtosis Anisotropy");
pub const FractionalOccupancySegmentation: Code<'static> = c("110855", "Fractional Occupancy Segmentation");
pub const FractionalProbabilisticSegmentation: Code<'static> = c("110854", "Fractional Probabilistic Segmentation");
pub const FractionationBasedOrTemporallyBasedMethods: Code<'static> = c("130127", "Fractionation-based or temporally-based methods");
pub const FrameExtractingEquipment: Code<'static> = c("109105", "Frame Extracting Equipment");
pub const FrameOfReferenceIdentity: Code<'static> = c("125021", "Frame of Reference Identity");
pub const FrameOfReferenceUID: Code<'static> = c("112227", "Frame of Reference UID");
pub const FrameToFrameAnalysis: Code<'static> = c("122499", "Frame to Frame Analysis");
pub const FraminghamQTcAlgorithm: Code<'static> = c("122733", "Framingham QTc Algorithm");
pub const FreeAcquisition: Code<'static> = c("113807", "Free Acquisition");
pub const FreeCordLoopMethod: Code<'static> = c("131020", "Free Cord Loop Method");
pub const FreeSilicone: Code<'static> = c("111508", "Free silicone");
pub const Freehand: Code<'static> = c("114215", "Freehand");
pub const FrequencyOfStimulusEvents: Code<'static> = c("130495", "Frequency of Stimulus Events");
pub const Fresolimumab89Zr: Code<'static> = c("126748", "Fresolimumab ^89^Zr");
pub const FridericiaQTcAlgorithm: Code<'static> = c("122732", "Fridericia QTc Algorithm");
pub const FrontOfCorneaToFrontOfLens: Code<'static> = c("111776", "Front Of Cornea To Front Of Lens");
pub const FullBernoulli: Code<'static> = c("125217", "Full Bernoulli");
pub const FullCardiacCycle: Code<'static> = c("125324", "Full Cardiac Cycle");
pub const FullFidelityImage: Code<'static> = c("121327", "Full fidelity image");
pub const FunctionalConditionPresentDuringAcquisition: Code<'static> = c("130324", "Functional condition present during acquisition");
pub const FunctionalImage: Code<'static> = c("121339", "Functional image");
#[deprecated = "retired in PS3.16"]
pub const Fundoscopy: Code<'static> = c("FS", "Fundoscopy");
pub const GA20189Zr: Code<'static> = c("126731", "GA201 ^89^Zr");
pub const GDPattern: Code<'static> = c("109903", "GD Pattern");
pub const GGraft: Code<'static> = c("130734", "G - Graft");
pub const GTVNodal: Code<'static> = c("130051", "GTV Nodal");
pub const GTVPrimary: Code<'static> = c("130052", "GTV Primary");
#[deprecated = "retired in PS3.16"]
pub const Galactocele: Code<'static> = c("111269", "Galactocele");
pub const GammaCapillaryTransitTimeModel: Code<'static> = c("126345", "Gamma Capillary Transit Time Model");
pub const GammaDistributionMode: Code<'static> = c("113297", "Gamma Distribution Mode");
pub const GammaDistributionModel: Code<'static> = c("113253", "Gamma distribution model");
pub const GammaDistributionScaleParameter: Code<'static> = c("113295", "Gamma Distribution Scale Parameter");
pub const GammaDistributionShapeParameter: Code<'static> = c("113296", "Gamma Distribution Shape Parameter");
pub const GanglionCellAndInnerPlexiformLayers: Code<'static> = c("131299", "Ganglion cell and inner plexiform layers");
pub const GanglionCellComplex: Code<'static> = c("131300", "Ganglion cell complex");
pub const GanglionCellComplexThickness: Code<'static> = c("111926", "Ganglion cell complex thickness");
pub const GanglionCellLayerKeyMeasurements: Code<'static> = c("131244", "Ganglion Cell Layer Key Measurements");
pub const GarwayHeathSectors: Code<'static> = c("131305", "Garway-Heath sectors");
pub const GastrointestinalImagingSubjectMatter: Code<'static> = c("128731", "Gastrointestinal imaging subject matter");
pub const GaussianBlur: Code<'static> = c("113088", "Gaussian blur");
pub const GaussianFilter: Code<'static> = c("130764", "Gaussian filter");
pub const GeneralMicroscopy: Code<'static> = c("GM", "General Microscopy");
pub const GeneralReductionInSensitivity: Code<'static> = c("111850", "General reduction in sensitivity");
pub const Generic2DPlanning: Code<'static> = c("112340", "Generic 2D Planning");
pub const Generic3DPlanning: Code<'static> = c("112341", "Generic 3D Planning");
pub const GenericPlanningForHipReplacement: Code<'static> = c("112342", "Generic Planning for Hip Replacement");
pub const GenericPlanningForKneeReplacement: Code<'static> = c("112343", "Generic Planning for Knee Replacement");
pub const GeneticModifications: Code<'static> = c("127414", "Genetic modifications");
pub const GeneticModificationsDescription: Code<'static> = c("127415", "Genetic modifications description");
pub const GenitourinaryHistory: Code<'static> = c("130548", "Genitourinary History");
pub const GenitourinaryImagingSubjectMatter: Code<'static> = c("128732", "Genitourinary imaging subject matter");
pub const GeographicAtrophyArea: Code<'static> = c("131310", "Geographic atrophy area");
pub const GeometricCenterpoint: Code<'static> = c("128137", "Geometric Centerpoint");
pub const GeometricIsocenter: Code<'static> = c("122486", "Geometric Isocenter");
pub const GeometricNonIsocenter: Code<'static> = c("122487", "Geometric Non-Isocenter");
pub const GeometricPurposeOfRegion: Code<'static> = c("130400", "Geometric purpose of region");
pub const GeometricRadiationTransportModel: Code<'static> = c("128421", "Geometric Radiation Transport Model");
pub const GeometricalImageEvaluation: Code<'static> = c("109708", "Geometrical image evaluation");
pub const GlaucomaHemifieldTestAnalysis: Code<'static> = c("111855", "Glaucoma Hemifield Test Analysis");
pub const GlembatumumabVedotin89Zr: Code<'static> = c("126724", "Glembatumumab vedotin ^89^Zr");
pub const Global: Code<'static> = c("113213", "Global");
pub const GlomerularFiltrationRateBlackMDRD: Code<'static> = c("113574", "Glomerular Filtration Rate black (MDRD)");
pub const GlomerularFiltrationRateCreatinineBasedFormulaSchwartz: Code<'static> = c("113577", "Glomerular Filtration Rate Creatinine-based formula (Schwartz)");
pub const GlomerularFiltrationRateCystatinBasedFormula: Code<'static> = c("113576", "Glomerular Filtration Rate Cystatin-based formula");
pub const GlomerularFiltrationRateFemaleMDRD: Code<'static> = c("113575", "Glomerular Filtration Rate female (MDRD)");
pub const GlomerularFiltrationRateMDRD: Code<'static> = c("113572", "Glomerular Filtration Rate (MDRD)");
pub const GlomerularFiltrationRateNonBlackMDRD: Code<'static> = c("113573", "Glomerular Filtration Rate non-black (MDRD)");
pub const GlucoseC11: Code<'static> = c("126521", "Glucose C^11^");
pub const GlucoseMeasurementDate: Code<'static> = c("127857", "Glucose Measurement Date");
pub const GlucoseMeasurementTime: Code<'static> = c("127858", "Glucose Measurement Time");
pub const GlutamateAndGlutamine: Code<'static> = c("113080", "Glutamate and glutamine");
pub const GlutamineC11: Code<'static> = c("126709", "Glutamine C^11^");
pub const GlutamineC14: Code<'static> = c("126710", "Glutamine C^14^");
pub const GlutamineF18: Code<'static> = c("126711", "Glutamine F^18^");
pub const Glycolysis: Code<'static> = c("126034", "Glycolysis");
pub const GoodImageQuality: Code<'static> = c("122741", "Good image quality");
pub const GradientAssessment: Code<'static> = c("122123", "Gradient assessment");
pub const GradientPressureAverage: Code<'static> = c("122197", "Gradient pressure, average");
pub const GradientPressurePeak: Code<'static> = c("122198", "Gradient pressure, peak");
pub const GradientWeightedClassActivation: Code<'static> = c("130403", "Gradient-weighted class activation");
pub const GraftToCitedSegmentDistalSection: Code<'static> = c("122104", "Graft to cited segment, distal section");
pub const GraftToCitedSegmentMidSection: Code<'static> = c("122103", "Graft to cited segment, mid section");
pub const GraftToCitedSegmentProximalSection: Code<'static> = c("122102", "Graft to cited segment, proximal section");
pub const GraftType: Code<'static> = c("125102", "Graft Type");
pub const GranularPattern: Code<'static> = c("112128", "Granular pattern");
pub const GraphIncrement: Code<'static> = c("122511", "Graph Increment");
pub const Graticule: Code<'static> = c("130349", "Graticule");
pub const GrayLevelCoOccurrenceMatrix: Code<'static> = c("126067", "Gray Level Co-occurrence Matrix");
pub const GrayLevelDistanceZoneMatrix: Code<'static> = c("128778", "Gray Level Distance Zone Matrix");
#[deprecated = "retired in PS3.16"]
pub const GrayLevelNonuniformityInRuns: Code<'static> = c("128809", "Gray Level Nonuniformity in Runs");
#[deprecated = "retired in PS3.16"]
pub const GrayLevelNonuniformityInRunsNormalized: Code<'static> = c("128810", "Gray Level Nonuniformity in Runs Normalized");
#[deprecated = "retired in PS3.16"]
pub const GrayLevelNonuniformityOfZoneCounts: Code<'static> = c("128829", "Gray Level Nonuniformity of Zone Counts");
#[deprecated = "retired in PS3.16"]
pub const GrayLevelNonuniformityOfZoneCountsNormalized: Code<'static> = c("128830", "Gray Level Nonuniformity of Zone Counts Normalized");
pub const GrayLevelRunLengthMatrix: Code<'static> = c("128776", "Gray Level Run Length Matrix");
pub const GrayLevelSizeZoneMatrix: Code<'static> = c("128777", "Gray Level Size Zone Matrix");
#[deprecated = "retired in PS3.16"]
pub const GrayLevelVarianceInRuns: Code<'static> = c("128814", "Gray Level Variance in Runs");
#[deprecated = "retired in PS3.16"]
pub const GrayLevelVarianceInZones: Code<'static> = c("128834", "Gray Level Variance in Zones");
pub const GrayscaleResolutionEvaluation: Code<'static> = c("109702", "Grayscale resolution evaluation");
pub const GreaterThan1YearAgo: Code<'static> = c("111398", "> 1 year ago");
#[deprecated = "retired in PS3.16"]
pub const GreenFilter: Code<'static> = c("111601", "Green filter");
pub const GridArtifactS: Code<'static> = c("111208", "Grid artifact(s)");
pub const GridBScanPattern: Code<'static> = c("128288", "Grid B-scan pattern");
#[deprecated = "retired in PS3.16"]
pub const GrossSpecimenImaging: Code<'static> = c("112701", "Gross Specimen Imaging");
pub const GroundGlassOpacity: Code<'static> = c("112120", "Ground glass opacity");
pub const GroupOfFramesForDisplay: Code<'static> = c("113036", "Group of Frames for Display");
pub const GrowthPercentileRank: Code<'static> = c("125012", "Growth Percentile rank");
pub const GrowthZScore: Code<'static> = c("125013", "Growth Z-score");
pub const GruesomeContent: Code<'static> = c("131394", "Gruesome Content");
pub const GuidewireCrossingLesionSuccessful: Code<'static> = c("122302", "Guidewire crossing lesion successful");
pub const GuidewireCrossingLesionUnsuccessful: Code<'static> = c("122301", "Guidewire crossing lesion unsuccessful");
pub const GynecologicCondition: Code<'static> = c("111567", "Gynecologic condition");
pub const GynecologicSurgery: Code<'static> = c("111568", "Gynecologic surgery");
pub const HARDI: Code<'static> = c("113221", "HARDI");
pub const HISBundleWave: Code<'static> = c("109040", "HIS bundle wave");
pub const HabitualVisualAcuity: Code<'static> = c("111686", "Habitual Visual Acuity");
pub const Haigis: Code<'static> = c("111760", "Haigis");
pub const HaigisA0: Code<'static> = c("111769", "Haigis a0");
pub const HaigisA1: Code<'static> = c("111770", "Haigis a1");
pub const HaigisA2: Code<'static> = c("111771", "Haigis a2");
pub const HaigisL: Code<'static> = c("111761", "Haigis-L");
pub const HaigisLToric: Code<'static> = c("111861", "Haigis-L Toric");
pub const HaigisToric: Code<'static> = c("111860", "Haigis Toric");
pub const HalfTimeForTissueRepair: Code<'static> = c("130091", "Half-time for Tissue Repair");
pub const HalfValueLayer: Code<'static> = c("111634", "Half Value Layer");
pub const HaloSign: Code<'static> = c("112073", "Halo sign");
pub const HandGrips: Code<'static> = c("130854", "Hand Grips");
pub const HandGripsPresence: Code<'static> = c("130851", "Hand Grips Presence");
pub const HardCopy: Code<'static> = c("HC", "Hard Copy");
pub const HardCopyPrintServer: Code<'static> = c("PRINT", "Hard Copy Print Server");
pub const HardWedge: Code<'static> = c("130346", "Hard Wedge");
pub const HardwareConfiguration: Code<'static> = c("110130", "Hardware Configuration");
pub const HeadAndNeckImagingSpecialty: Code<'static> = c("128008", "Head and Neck Imaging Specialty");
pub const HeadAndNeckMask: Code<'static> = c("130112", "Head and Neck Mask");
pub const HeadFixationBoard: Code<'static> = c("130656", "Head Fixation Board");
pub const HeadFootAxis: Code<'static> = c("110861", "Head-Foot Axis");
pub const HeadMask: Code<'static> = c("130111", "Head Mask");
pub const HeadNodeSet: Code<'static> = c("130362", "Head Node Set");
pub const HeadOfCardiology: Code<'static> = c("128675", "Head of Cardiology");
pub const HeadOfRadiology: Code<'static> = c("128670", "Head of Radiology");
pub const HeadToFoot: Code<'static> = c("110868", "Head To Foot");
pub const Headframe: Code<'static> = c("130110", "Headframe");
pub const HeadframePresent: Code<'static> = c("130460", "Headframe Present");
pub const HeartValveFlail: Code<'static> = c("127856", "Heart valve flail");
pub const HeatedImagingDevice: Code<'static> = c("127251", "Heated imaging device");
pub const HeatedPatientSupport: Code<'static> = c("127252", "Heated patient support");
pub const HeatedWaterBlanket: Code<'static> = c("127253", "Heated water blanket");
pub const HeatingConditions: Code<'static> = c("127040", "Heating conditions");
pub const HeelStop: Code<'static> = c("130853", "Heel Stop");
pub const HeelStopPosition: Code<'static> = c("130844", "Heel Stop Position");
pub const Height: Code<'static> = c("121207", "Height");
pub const HelicalBeam: Code<'static> = c("130108", "Helical Beam");
#[deprecated = "retired in PS3.16"]
pub const HemangiomaNonparenchymalSubcutaneous: Code<'static> = c("111271", "Hemangioma - nonparenchymal, subcutaneous");
pub const HemisphericalUltrasoundTransducerGeometry: Code<'static> = c("130809", "Hemispherical ultrasound transducer geometry");
pub const Hemodynamic: Code<'static> = c("HD", "Hemodynamic");
pub const HemodynamicResistance: Code<'static> = c("110825", "Hemodynamic Resistance");
pub const HemodynamicResistanceIndex: Code<'static> = c("122220", "Hemodynamic Resistance Index");
pub const HemodynamicsReport: Code<'static> = c("122120", "Hemodynamics Report");
pub const HemostasisAchieved: Code<'static> = c("122033", "Hemostasis achieved");
pub const HemostasisNotAchievedActivelyBleeding: Code<'static> = c("122035", "Hemostasis not achieved - actively bleeding");
pub const HemostasisNotAchievedOozing: Code<'static> = c("122034", "Hemostasis not achieved - oozing");
pub const Hermaphrodite: Code<'static> = c("H", "Hermaphrodite");
pub const HerniatedImplant: Code<'static> = c("111509", "Herniated implant");
pub const HeterogeneousEchotexture: Code<'static> = c("111353", "Heterogeneous echotexture");
pub const Hiatus: Code<'static> = c("112095", "Hiatus");
pub const High: Code<'static> = c("111575", "High");
pub const HighDoseFractionLinearCellKillFactor: Code<'static> = c("130090", "High Dose Fraction Linear Cell Kill Factor");
pub const HighDoseFractionTransitionDose: Code<'static> = c("130092", "High Dose Fraction Transition Dose");
#[deprecated = "retired in PS3.16"]
pub const HighGrayLevelRunEmphasis: Code<'static> = c("128804", "High Gray Level Run Emphasis");
#[deprecated = "retired in PS3.16"]
pub const HighGrayLevelZoneEmphasis: Code<'static> = c("128824", "High Gray Level Zone Emphasis");
pub const HighResolution: Code<'static> = c("114210", "High resolution");
pub const HighShortAxisView: Code<'static> = c("131378", "High short axis view");
pub const HighThresholdEmptyTileSuppression: Code<'static> = c("112720", "High threshold empty tile suppression");
pub const HighThyroidUptake: Code<'static> = c("113562", "High Thyroid Uptake");
pub const HighlySuggestiveOfMalignancyTakeAppropriateAction: Code<'static> = c("111149", "Highly suggestive of malignancy - take appropriate action");
pub const HillRBF30: Code<'static> = c("111876", "Hill RBF 3.0");
pub const HisBundleElectrogram: Code<'static> = c("109007", "His bundle electrogram");
pub const HistogramAnalysis: Code<'static> = c("123105", "Histogram Analysis");
pub const HistologyUsingCoreBiopsy: Code<'static> = c("111145", "Histology using core biopsy");
pub const HistoricalRTPrescription: Code<'static> = c("130135", "Historical RT Prescription");
#[deprecated = "retired in PS3.16"]
pub const History: Code<'static> = c("121060", "History");
pub const HistoryOfBenignBreastBiopsy: Code<'static> = c("111420", "History of benign breast biopsy");
pub const HistoryOfBreastAugmentationAsymptomatic: Code<'static> = c("111417", "History of breast augmentation, asymptomatic");
pub const HistoryOfEndometrialCancer: Code<'static> = c("111551", "History of endometrial cancer");
pub const HistoryOfHighRiskLesionOnPreviousBiopsy: Code<'static> = c("111553", "History of high risk lesion on previous biopsy");
pub const HistoryOfMultipleFetuses: Code<'static> = c("111572", "History of multiple fetuses");
pub const HistoryOfNonMelanomaSkinCancer: Code<'static> = c("130482", "History of non-melanoma skin cancer");
pub const HistoryOfOvarianCancer: Code<'static> = c("111552", "History of ovarian cancer");
pub const HodgesQTcAlgorithm: Code<'static> = c("122731", "Hodges QTc Algorithm");
pub const HofferPACDConstant: Code<'static> = c("111772", "Hoffer pACD Constant");
pub const HofferQ: Code<'static> = c("111764", "Hoffer Q");
pub const Holladay1: Code<'static> = c("111762", "Holladay 1");
pub const Holladay2: Code<'static> = c("111763", "Holladay 2");
pub const HomogeneityFactor: Code<'static> = c("128455", "Homogeneity Factor");
pub const Homogeneous: Code<'static> = c("112160", "Homogeneous");
pub const HomogeneousFatEchotexture: Code<'static> = c("111351", "Homogeneous fat echotexture");
pub const HomogeneousFibroglandularEchotexture: Code<'static> = c("111352", "Homogeneous fibroglandular echotexture");
pub const HoneycombPattern: Code<'static> = c("112106", "Honeycomb pattern");
pub const HorizontalPixelSpacing: Code<'static> = c("111026", "Horizontal Pixel Spacing");
pub const HorizontalPrismBase: Code<'static> = c("111676", "Horizontal Prism Base");
pub const HorizontalPrismPower: Code<'static> = c("111675", "Horizontal Prism Power");
pub const HousingChangeInterval: Code<'static> = c("127151", "Housing change interval");
pub const HousingIndividuallyVentilated: Code<'static> = c("127170", "Housing individually ventilated");
pub const HousingManufacturer: Code<'static> = c("127125", "Housing manufacturer");
pub const HousingRackProductCode: Code<'static> = c("127127", "Housing rack product code");
pub const HousingRackProductName: Code<'static> = c("127126", "Housing rack product name");
pub const HousingRole: Code<'static> = c("127110", "Housing role");
pub const HousingUnitHeight: Code<'static> = c("127161", "Housing unit height");
pub const HousingUnitLength: Code<'static> = c("127162", "Housing unit length");
pub const HousingUnitLidProductCode: Code<'static> = c("127131", "Housing unit lid product code");
pub const HousingUnitLidProductName: Code<'static> = c("127130", "Housing unit lid product name");
pub const HousingUnitLocationInRack: Code<'static> = c("127142", "Housing unit location in rack");
pub const HousingUnitMovement: Code<'static> = c("127153", "Housing unit movement");
pub const HousingUnitProductCode: Code<'static> = c("127129", "Housing unit product code");
pub const HousingUnitProductName: Code<'static> = c("127128", "Housing unit product name");
pub const HousingUnitReuse: Code<'static> = c("127175", "Housing unit reuse");
pub const HousingUnitWidth: Code<'static> = c("127160", "Housing unit width");
pub const HumanAlphaSynucleinPreformedFibrils: Code<'static> = c("127851", "Human alpha synuclein preformed fibrils");
pub const HumanTauPreformedFibrils: Code<'static> = c("127853", "Human Tau preformed fibrils");
pub const HydrationVolume: Code<'static> = c("113551", "Hydration Volume");
pub const HydropsFetalisScore: Code<'static> = c("131031", "Hydrops Fetalis Score");
pub const HyperAcute: Code<'static> = c("112159", "Hyper-acute");
pub const HyperFractionation: Code<'static> = c("130099", "Hyper-fractionation");
pub const Hyperechoic: Code<'static> = c("111362", "Hyperechoic");
pub const HyperemicRatio: Code<'static> = c("122219", "Hyperemic ratio");
#[deprecated = "retired in PS3.16"]
pub const HyperplasiaUsual: Code<'static> = c("111273", "Hyperplasia, usual");
pub const HyperventilationBegin: Code<'static> = c("130413", "Hyperventilation begin");
pub const HyperventilationEnd: Code<'static> = c("130414", "Hyperventilation end");
pub const HypoFractionation: Code<'static> = c("130098", "Hypo-fractionation");
pub const Hypoechoic: Code<'static> = c("111364", "Hypoechoic");
pub const HzAngular: Code<'static> = c("130775", "Hz angular");
pub const HzNormalized: Code<'static> = c("130774", "Hz normalized");
pub const IAUC: Code<'static> = c("126320", "IAUC");
pub const IAUC180: Code<'static> = c("126323", "IAUC180");
pub const IAUC180BN: Code<'static> = c("126327", "IAUC180BN");
pub const IAUC60: Code<'static> = c("126321", "IAUC60");
pub const IAUC60BN: Code<'static> = c("126325", "IAUC60BN");
pub const IAUC90: Code<'static> = c("126322", "IAUC90");
pub const IAUC90BN: Code<'static> = c("126326", "IAUC90BN");
pub const IAUCBN: Code<'static> = c("126324", "IAUCBN");
pub const ICRPPub103: Code<'static> = c("113841", "ICRP Pub 103");
pub const ICRPPub60: Code<'static> = c("113808", "ICRP Pub 60");
pub const ICRPPublication106: Code<'static> = c("113523", "ICRP Publication 106");
pub const ICRPPublication128: Code<'static> = c("113524", "ICRP Publication 128");
pub const ICRPPublication53: Code<'static> = c("113521", "ICRP Publication 53");
pub const ICRPPublication80: Code<'static> = c("113522", "ICRP Publication 80");
#[deprecated = "retired in PS3.16"]
pub const IDPlate: Code<'static> = c("111174", "ID Plate");
pub const IEC160mmHeadCTDosimetryPhantom: Code<'static> = c("113690", "IEC 160mm Head CT Dosimetry Phantom");
pub const IEC320mmBodyCTDosimetryPhantom: Code<'static> = c("113691", "IEC 320mm Body CT Dosimetry Phantom");
pub const IEC61217GantryContinuousPitchAngle: Code<'static> = c("126810", "IEC61217 Gantry Continuous Pitch Angle");
pub const IEC61217GantryContinuousRollAngle: Code<'static> = c("126809", "IEC61217 Gantry Continuous Roll Angle");
pub const IEC61217GantryContinuousYawAngle: Code<'static> = c("126811", "IEC61217 Gantry Continuous Yaw Angle");
pub const IEC61217ImagingSourceToAxisDistance: Code<'static> = c("130801", "IEC61217 Imaging Source to Axis Distance");
pub const IEC61217PatientSupportContinuousYawAngle: Code<'static> = c("126801", "IEC61217 Patient Support Continuous Yaw Angle");
pub const IEC61217TableTopContinuousEccentricAngle: Code<'static> = c("126805", "IEC61217 Table Top Continuous Eccentric Angle");
pub const IEC61217TableTopContinuousPitchAngle: Code<'static> = c("126802", "IEC61217 Table Top Continuous Pitch Angle");
pub const IEC61217TableTopContinuousRollAngle: Code<'static> = c("126803", "IEC61217 Table Top Continuous Roll Angle");
pub const IEC61217TableTopEccentricAxisDistance: Code<'static> = c("126804", "IEC61217 Table Top Eccentric Axis Distance");
pub const IEC61217TableTopLateralPosition: Code<'static> = c("126806", "IEC61217 Table Top Lateral Position");
pub const IEC61217TableTopLongitudinalPosition: Code<'static> = c("126807", "IEC61217 Table Top Longitudinal Position");
pub const IEC61217TableTopVerticalPosition: Code<'static> = c("126808", "IEC61217 Table Top Vertical Position");
pub const IEC61217XRayImageReceptorLateralDisplacement: Code<'static> = c("130804", "IEC61217 X-Ray Image Receptor Lateral Displacement");
pub const IEC61217XRayImageReceptorLongitudinalDisplacement: Code<'static> = c("130803", "IEC61217 X-Ray Image Receptor Longitudinal Displacement");
pub const IEC61217XRayImageReceptorRadialDisplacementFromIsocenter: Code<'static> = c("130802", "IEC61217 X-Ray Image Receptor Radial Displacement from Isocenter");
pub const IEC61217XRayImageReceptorRotation: Code<'static> = c("130805", "IEC61217 X-Ray Image Receptor Rotation");
pub const IIRFilter: Code<'static> = c("130772", "IIR filter");
pub const ILCR: Code<'static> = c("126850", "ILCR");
pub const IRIGBTimecode: Code<'static> = c("109003", "IRIG-B timecode");
pub const ISUOG1stTrimester2023: Code<'static> = c("131380", "ISUOG 1st Trimester 2023");
pub const ISUOG2ndTrimester2022: Code<'static> = c("131381", "ISUOG 2nd Trimester 2022");
pub const ISUOG3rdTrimester2024: Code<'static> = c("131382", "ISUOG 3rd Trimester 2024");
pub const ITV: Code<'static> = c("130056", "ITV");
pub const IVCAWavePeakVelocity: Code<'static> = c("131063", "IVC a-wave peak velocity");
pub const IVCPreloadIndex: Code<'static> = c("131011", "IVC preload index");
pub const IVCSPerA: Code<'static> = c("131012", "IVC S/a");
pub const IVCSWavePeakVelocity: Code<'static> = c("131062", "IVC S-wave peak velocity");
pub const IVUSReport: Code<'static> = c("122325", "IVUS Report");
pub const IdentifiableEffectOnSurroundingTissues: Code<'static> = c("111371", "Identifiable effect on surrounding tissues");
pub const IdentificationOfTheAttenuator: Code<'static> = c("130527", "Identification of the Attenuator");
pub const IdentificationOfTheXRaySource: Code<'static> = c("113832", "Identification of the X-Ray Source");
pub const Identifier: Code<'static> = c("125010", "Identifier");
pub const IdentifierWithinPersonObserversRole: Code<'static> = c("128775", "Identifier within Person Observer's Role");
pub const IdentifyingContent: Code<'static> = c("131393", "Identifying Content");
pub const IdentifyingSegment: Code<'static> = c("112229", "Identifying Segment");
pub const IfAgeGreaterThan60yP50Equals293: Code<'static> = c("122261", "if (age > 60y) P50 = 29.3");
pub const IfAgeLessThan18yP50Equals272: Code<'static> = c("122259", "if (age < 18y) P50 = 27.2");
pub const IfAgeLessThan1dayP50Equals216: Code<'static> = c("122257", "if (age < 1day) P50 = 21.6");
pub const IfAgeLessThan30dayP50Equals246: Code<'static> = c("122258", "if (age < 30day) P50 = 24.6");
pub const IfAgeLessThan40yP50Equals274: Code<'static> = c("122260", "if (age < 40y) P50 = 27.4");
pub const IfPremAgeLessThan3daysP50Equals199: Code<'static> = c("122256", "if (prem age< 3days) P50 = 19.9");
pub const Igloo: Code<'static> = c("127220", "Igloo");
pub const IlluminationMethod: Code<'static> = c("112706", "Illumination Method");
pub const IlluminationSpectralBand: Code<'static> = c("112711", "Illumination spectral band");
pub const IlluminationWavelength: Code<'static> = c("112710", "Illumination wavelength");
pub const IllustrationOfFinding: Code<'static> = c("125201", "Illustration of Finding");
pub const IllustrationOfROI: Code<'static> = c("121200", "Illustration of ROI");
pub const ImageAcquired: Code<'static> = c("121138", "Image Acquired");
pub const ImageAcquiredDuringTreatment: Code<'static> = c("129203", "Image Acquired during Treatment");
pub const ImageArtifactS: Code<'static> = c("111207", "Image artifact(s)");
pub const ImageCapture: Code<'static> = c("CAPTURE", "Image Capture");
pub const ImageContentAndFiducialBasedAlignment: Code<'static> = c("125026", "Image Content and Fiducial Based Alignment");
pub const ImageContentBasedAlignment: Code<'static> = c("125024", "Image Content-based Alignment");
pub const ImageDetectedMass: Code<'static> = c("111126", "Image detected mass");
pub const ImageDetector: Code<'static> = c("130351", "Image Detector");
pub const ImageFrame: Code<'static> = c("130781", "Image frame");
pub const ImageInterval: Code<'static> = c("122640", "Image Interval");
pub const ImageLaterality: Code<'static> = c("111027", "Image Laterality");
pub const ImageLibrary: Code<'static> = c("111028", "Image Library");
pub const ImageLibraryGroup: Code<'static> = c("126200", "Image Library Group");
pub const ImageOrientationPatientColumnX: Code<'static> = c("110907", "Image Orientation (Patient) Column X");
pub const ImageOrientationPatientColumnY: Code<'static> = c("110908", "Image Orientation (Patient) Column Y");
pub const ImageOrientationPatientColumnZ: Code<'static> = c("110909", "Image Orientation (Patient) Column Z");
pub const ImageOrientationPatientRowX: Code<'static> = c("110904", "Image Orientation (Patient) Row X");
pub const ImageOrientationPatientRowY: Code<'static> = c("110905", "Image Orientation (Patient) Row Y");
pub const ImageOrientationPatientRowZ: Code<'static> = c("110906", "Image Orientation (Patient) Row Z");
pub const ImagePositionPatientX: Code<'static> = c("110901", "Image Position (Patient) X");
pub const ImagePositionPatientY: Code<'static> = c("110902", "Image Position (Patient) Y");
pub const ImagePositionPatientZ: Code<'static> = c("110903", "Image Position (Patient) Z");
pub const ImageProcessing: Code<'static> = c("110001", "Image Processing");
pub const ImageQuality: Code<'static> = c("111101", "Image Quality");
pub const ImageQualityController: Code<'static> = c("129003", "Image Quality Controller");
pub const ImageQualityRating: Code<'static> = c("111029", "Image Quality Rating");
pub const ImageQualityReferenceParameters: Code<'static> = c("131550", "Image Quality Reference Parameters");
pub const ImageReconstruction: Code<'static> = c("110014", "Image Reconstruction");
pub const ImageRegion: Code<'static> = c("111030", "Image Region");
pub const ImageSetProperties: Code<'static> = c("112224", "Image Set Properties");
pub const ImageSetQualityRating: Code<'static> = c("111694", "Image Set Quality Rating");
pub const ImageSpaceMAR: Code<'static> = c("131541", "Image-Space MAR");
pub const ImageSpecificity: Code<'static> = c("111091", "Image Specificity");
pub const ImageStitching: Code<'static> = c("113090", "Image stitching");
pub const ImageType: Code<'static> = c("121141", "Image Type");
pub const ImageUsedAsReferenceImageForTreatment: Code<'static> = c("129204", "Image used as Reference Image for Treatment");
pub const ImageUsedForDoseCalculation: Code<'static> = c("129202", "Image used for Dose Calculation");
pub const ImageUsedForTreatmentPlanning: Code<'static> = c("129201", "Image used for Treatment Planning");
pub const ImageView: Code<'static> = c("111031", "Image View");
pub const ImageViewModifier: Code<'static> = c("111032", "Image View Modifier");
pub const ImagedContainerLabel: Code<'static> = c("121354", "Imaged container label");
pub const ImagesRelatedToStandaloneObject: Code<'static> = c("121316", "Images related to standalone object");
pub const Imagify: Code<'static> = c("125903", "Imagify");
pub const ImagingAgentAdministrationActivity: Code<'static> = c("130237", "Imaging Agent Administration Activity");
pub const ImagingAgentAdministrationAdverseEvents: Code<'static> = c("130212", "Imaging Agent Administration Adverse Events");
pub const ImagingAgentAdministrationCompletionStatus: Code<'static> = c("130211", "Imaging Agent Administration Completion Status");
pub const ImagingAgentAdministrationConsumable: Code<'static> = c("130222", "Imaging Agent Administration Consumable");
pub const ImagingAgentAdministrationConsumableType: Code<'static> = c("130223", "Imaging Agent Administration Consumable Type");
pub const ImagingAgentAdministrationDelay: Code<'static> = c("130197", "Imaging Agent Administration Delay");
pub const ImagingAgentAdministrationGraph: Code<'static> = c("130232", "Imaging Agent Administration Graph");
pub const ImagingAgentAdministrationInjectorEventType: Code<'static> = c("130234", "Imaging Agent Administration Injector Event Type");
pub const ImagingAgentAdministrationInjectorEvents: Code<'static> = c("130233", "Imaging Agent Administration Injector Events");
pub const ImagingAgentAdministrationInjectorPhaseIdentifier: Code<'static> = c("130264", "Imaging Agent Administration Injector Phase Identifier");
pub const ImagingAgentAdministrationPerformedPhaseUID: Code<'static> = c("130261", "Imaging Agent Administration Performed Phase UID");
pub const ImagingAgentAdministrationPerformedStepUID: Code<'static> = c("130246", "Imaging Agent Administration Performed Step UID");
pub const ImagingAgentAdministrationPhase: Code<'static> = c("130202", "Imaging Agent Administration Phase");
pub const ImagingAgentAdministrationPhaseIdentifier: Code<'static> = c("130203", "Imaging Agent Administration Phase Identifier");
pub const ImagingAgentAdministrationPhaseType: Code<'static> = c("130204", "Imaging Agent Administration Phase Type");
pub const ImagingAgentAdministrationPhaseWithManualHold: Code<'static> = c("130265", "Imaging Agent Administration Phase with Manual Hold");
pub const ImagingAgentAdministrationStep: Code<'static> = c("130195", "Imaging Agent Administration Step");
pub const ImagingAgentAdministrationStepIdentifier: Code<'static> = c("130196", "Imaging Agent Administration Step Identifier");
pub const ImagingAgentAdministrationStepSequenceNumber: Code<'static> = c("130445", "Imaging Agent Administration Step Sequence Number");
pub const ImagingAgentAdministrationSteps: Code<'static> = c("130192", "Imaging Agent Administration Steps");
pub const ImagingAgentAdministrationStepsDescription: Code<'static> = c("130199", "Imaging Agent Administration Steps Description");
pub const ImagingAgentAdministrationStepsName: Code<'static> = c("130200", "Imaging Agent Administration Steps Name");
pub const ImagingAgentComponent: Code<'static> = c("130238", "Imaging Agent Component");
pub const ImagingAgentComponentUsage: Code<'static> = c("130191", "Imaging Agent Component Usage");
pub const ImagingAgentIdentifier: Code<'static> = c("130254", "Imaging Agent Identifier");
pub const ImagingAgentInformation: Code<'static> = c("130183", "Imaging Agent Information");
pub const ImagingAgentVolumePerUnitOfPresentation: Code<'static> = c("130221", "Imaging Agent Volume per Unit of Presentation");
pub const ImagingAgentWarmed: Code<'static> = c("130187", "Imaging Agent Warmed");
pub const ImagingConditions: Code<'static> = c("123016", "Imaging Conditions");
pub const ImagingDirection: Code<'static> = c("123015", "Imaging Direction");
pub const ImagingMeasurementReport: Code<'static> = c("126000", "Imaging Measurement Report");
pub const ImagingMeasurements: Code<'static> = c("126010", "Imaging Measurements");
pub const ImagingProcedureRoom: Code<'static> = c("127372", "Imaging procedure room");
pub const ImagingSeriesQuality: Code<'static> = c("130550", "Imaging Series Quality");
pub const ImagingStartDateTime: Code<'static> = c("122712", "Imaging Start DateTime");
pub const ImagingStudyQuality: Code<'static> = c("130549", "Imaging Study Quality");
pub const ImpairedRenalFunction: Code<'static> = c("128144", "Impaired Renal Function");
pub const ImplantAssemblyTemplate: Code<'static> = c("112366", "Implant Assembly Template");
pub const ImplantComponentList: Code<'static> = c("112360", "Implant Component List");
pub const ImplantFabrication: Code<'static> = c("129016", "Implant Fabrication");
pub const ImplantFinding: Code<'static> = c("111407", "Implant finding");
pub const ImplantTemplate: Code<'static> = c("112348", "Implant Template");
pub const ImplantType: Code<'static> = c("111405", "Implant type");
pub const ImplantationPatientPositioning: Code<'static> = c("112368", "Implantation Patient Positioning");
pub const ImplantationPlan: Code<'static> = c("112345", "Implantation Plan");
pub const Import: Code<'static> = c("110107", "Import");
pub const Impression: Code<'static> = c("121073", "Impression");
pub const ImpressionDescription: Code<'static> = c("111033", "Impression Description");
#[deprecated = "retired in PS3.16"]
pub const Impressions: Code<'static> = c("121072", "Impressions");
pub const InDetectorPlane: Code<'static> = c("113941", "In Detector Plane");
pub const InFocus: Code<'static> = c("131503", "In focus");
pub const InHomeCage: Code<'static> = c("127101", "In home cage");
pub const InStentNeointima: Code<'static> = c("122357", "In-Stent Neointima");
pub const InStentNeointimalCrossSectionalArea: Code<'static> = c("122335", "In-Stent Neointimal Cross-Sectional Area");
pub const InStentNeointimalVolume: Code<'static> = c("122374", "In-Stent Neointimal Volume");
pub const InadequateCompression: Code<'static> = c("111196", "Inadequate compression");
pub const InadequateContrastEnhancement: Code<'static> = c("130580", "Inadequate contrast enhancement");
pub const InadequatelyStained: Code<'static> = c("131508", "Inadequately stained");
pub const InappropriateForTheDevice: Code<'static> = c("128618", "Inappropriate for the device");
pub const InappropriateForTheIndications: Code<'static> = c("128621", "Inappropriate for the indications");
pub const InappropriateImageProcessing: Code<'static> = c("111219", "Inappropriate image processing");
pub const InappropriatePatientOrientation: Code<'static> = c("130461", "Inappropriate Patient Orientation");
pub const InappropriatePatientPosition: Code<'static> = c("130462", "Inappropriate Patient Position");
pub const IncisionMade: Code<'static> = c("111440", "Incision made");
pub const IncompleteStudy: Code<'static> = c("130574", "Incomplete study");
pub const InconsistentWithLabelingOfTheDevice: Code<'static> = c("128622", "Inconsistent with labeling of the device");
pub const IncorrectModalityWorklistEntry: Code<'static> = c("113038", "Incorrect Modality Worklist Entry");
pub const IncorrectProcedureOrdered: Code<'static> = c("110502", "Incorrect procedure ordered");
pub const IncorrectSideOrdered: Code<'static> = c("110512", "Incorrect side ordered");
pub const IncorrectWorklistEntrySelected: Code<'static> = c("110514", "Incorrect worklist entry selected");
pub const IncrementalAcquisitionTriggering: Code<'static> = c("130796", "Incremental Acquisition Triggering");
pub const IndependentRadiationShield: Code<'static> = c("130641", "Independent radiation shield");
pub const Index: Code<'static> = c("121425", "Index");
pub const Indexed: Code<'static> = c("125313", "Indexed");
pub const IndexedHemodynamicResistance: Code<'static> = c("110826", "Indexed Hemodynamic Resistance");
pub const IndicatedProblem: Code<'static> = c("111533", "Indicated Problem");
pub const IndicationsForPharmacologicalStress: Code<'static> = c("122700", "Indications for Pharmacological Stress");
#[deprecated = "retired in PS3.16"]
pub const IndicationsForProcedure: Code<'static> = c("121109", "Indications for Procedure");
pub const IndicatorAppearanceTime: Code<'static> = c("122192", "Indicator appearance time");
pub const IndicatorMeanTransitTime: Code<'static> = c("109071", "Indicator mean transit time");
pub const IndirectDetector: Code<'static> = c("113949", "Indirect Detector");
#[deprecated = "retired in PS3.16"]
pub const IndividualCalcification: Code<'static> = c("111104", "Individual Calcification");
pub const IndividualImpressionPerRecommendation: Code<'static> = c("111034", "Individual Impression/\u{200b}Recommendation");
pub const IndividualImpressionPerRecommendationAnalysis: Code<'static> = c("111233", "Individual Impression/\u{200b}Recommendation Analysis");
pub const IneligibleForReimbursement: Code<'static> = c("128615", "Ineligible for reimbursement");
pub const InferiorToAnterior: Code<'static> = c("110875", "Inferior To Anterior");
pub const InferolateralToSuperomedial: Code<'static> = c("111432", "Inferolateral to superomedial");
pub const InferomedialToSuperolateral: Code<'static> = c("111433", "Inferomedial to superolateral");
pub const Infiltrate: Code<'static> = c("112121", "Infiltrate");
pub const InflatableEndorectalCoilFillSubstance: Code<'static> = c("130545", "Inflatable endorectal coil fill substance");
pub const InflowAngle: Code<'static> = c("122546", "Inflow Angle");
pub const InformationTechnology: Code<'static> = c("128173", "Information Technology");
pub const InformationUsedForPlanning: Code<'static> = c("112358", "Information used for planning");
#[deprecated = "retired in PS3.16"]
pub const InfraredFilter: Code<'static> = c("111606", "Infrared filter");
pub const InfraredMarker: Code<'static> = c("130645", "Infrared Marker");
pub const InfraredReflectorMarker: Code<'static> = c("129309", "Infrared Reflector Marker");
pub const InfusateEnd: Code<'static> = c("122088", "Infusate end");
pub const InfusateStart: Code<'static> = c("122087", "Infusate start");
pub const InfusionSystem: Code<'static> = c("113542", "Infusion System");
pub const Inhomogeneous: Code<'static> = c("112161", "Inhomogeneous");
pub const InitialVolumeOfImagingAgentInContainer: Code<'static> = c("130205", "Initial Volume of Imaging Agent in Container");
pub const InitiatorRoleID: Code<'static> = c("110156", "Initiator Role ID");
pub const InjectateTemperature: Code<'static> = c("122320", "Injectate Temperature");
pub const InjectateVolume: Code<'static> = c("122321", "Injectate Volume");
pub const InjectorEventDetectionDateTime: Code<'static> = c("130235", "Injector Event Detection DateTime");
pub const InletIncluded: Code<'static> = c("131019", "Inlet Included");
pub const InnerLimitsOfFuzzyMargin: Code<'static> = c("113662", "Inner limits of fuzzy margin");
pub const InnerSurfaceOfTheEllipsoidZone: Code<'static> = c("128320", "Inner surface of the ellipsoid zone");
pub const InnerSurfaceOfTheInterdigitationZone: Code<'static> = c("128323", "Inner surface of the interdigitation zone");
pub const InnerSurfaceOfTheRPE: Code<'static> = c("128297", "Inner surface of the RPE");
pub const InsideOperationalLimitsOfTheDevice: Code<'static> = c("128607", "Inside operational limits of the device");
pub const InstanceNumber: Code<'static> = c("113609", "Instance Number");
pub const InstancesImported: Code<'static> = c("110028", "Instances Imported");
pub const InstitutionallyApprovedEstimates: Code<'static> = c("113529", "Institutionally Approved Estimates");
pub const InstitutionallyDefinedQualityControlStandard: Code<'static> = c("111240", "Institutionally defined quality control standard");
pub const InstrumentApproach: Code<'static> = c("111431", "Instrument Approach");
pub const InsufficientImplantDisplacementIncorrect: Code<'static> = c("111206", "Insufficient implant displacement incorrect");
pub const InsufficientQualityForInterpretation: Code<'static> = c("110531", "Insufficient quality for interpretation");
pub const IntegratedProjectionRadiographySystem: Code<'static> = c("113958", "Integrated Projection Radiography System");
pub const IntegrationOfSumOfClosedAreasOnContiguousSlicesMethodForVolume: Code<'static> = c("122503", "Integration of sum of closed areas on contiguous slices method for volume");
pub const InterHemisphericPlane: Code<'static> = c("125030", "Inter-Hemispheric Plane");
pub const InterMarkerDistance: Code<'static> = c("121208", "Inter-Marker Distance");
pub const Interface: Code<'static> = c("112082", "Interface");
pub const InterferingTearsOrDrops: Code<'static> = c("111695", "Interfering Tears or Drops");
pub const InterferometricTransducer: Code<'static> = c("130817", "Interferometric Transducer");
pub const Interferometry: Code<'static> = c("114202", "Interferometry");
pub const InterferometryBasedCornealTomographer: Code<'static> = c("111947", "Interferometry-based corneal tomographer");
pub const InterlockOverridden: Code<'static> = c("130454", "Interlock Overridden");
pub const InterlockResolved: Code<'static> = c("130744", "Interlock Resolved");
pub const IntermediateFamilyHistoryOfBreastCancer: Code<'static> = c("111560", "Intermediate family history of breast cancer");
pub const IntermediateLevel: Code<'static> = c("128726", "Intermediate level");
#[deprecated = "retired in PS3.16"]
pub const Intern: Code<'static> = c("121085", "Intern");
pub const InternalOsDistance: Code<'static> = c("131530", "Internal Os Distance");
pub const InternationalProgressiveDogBreedersAlliance: Code<'static> = c("109214", "International Progressive Dog Breeders' Alliance");
pub const InterpolatedLocalReference: Code<'static> = c("122490", "Interpolated Local Reference");
pub const Interpretation: Code<'static> = c("110005", "Interpretation");
pub const InterpretationRequiresSpecialistExpertise: Code<'static> = c("110532", "Interpretation requires specialist expertise");
pub const InterquartileRangeOfPopulation: Code<'static> = c("130614", "Interquartile Range of population");
pub const InterquartileRangeOfUltrasoundAttenuationCoefficient: Code<'static> = c("131345", "Interquartile Range of Ultrasound Attenuation Coefficient");
pub const InterquartileRangeToMedianRatioOfPopulation: Code<'static> = c("130615", "Interquartile Range to Median Ratio of population");
pub const InterquartileRangeToMedianRatioOfUltrasoundAttenCoeff: Code<'static> = c("131346", "Interquartile Range to Median Ratio of Ultrasound Atten Coeff");
pub const InterstitialIllumination: Code<'static> = c("130814", "Interstitial illumination");
pub const InterventionAction: Code<'static> = c("122090", "Intervention Action");
pub const InterventionAttemptIdentifier: Code<'static> = c("121154", "Intervention attempt identifier");
pub const InterventionPerformed: Code<'static> = c("122312", "Intervention performed");
pub const InterventionalDevicePlaced: Code<'static> = c("122311", "Interventional device placed");
pub const InterventionalDevicePlacementUnsuccessful: Code<'static> = c("122310", "Interventional device placement unsuccessful");
pub const InterventionalDeviceWithdrawn: Code<'static> = c("122313", "Interventional device withdrawn");
pub const IntimalDissection: Code<'static> = c("122398", "Intimal Dissection");
pub const IntraOralRadiography: Code<'static> = c("IO", "Intra-oral Radiography");
pub const IntraProceduralStructuralHeartMeasurementReport: Code<'static> = c("131091", "Intra-procedural Structural Heart Measurement Report");
pub const IntraStentDissection: Code<'static> = c("122388", "Intra-stent Dissection");
pub const IntracysticLesion: Code<'static> = c("111461", "Intracystic lesion");
pub const IntracysticPapillaryCarcinoma: Code<'static> = c("111315", "Intracystic papillary carcinoma");
#[deprecated = "retired in PS3.16"]
pub const IntracysticPapilloma: Code<'static> = c("111257", "Intracystic papilloma");
pub const IntraductalCarcinomaHighGrade: Code<'static> = c("111341", "Intraductal carcinoma, high grade");
pub const IntraductalCarcinomaLowGrade: Code<'static> = c("111313", "Intraductal carcinoma, low grade");
#[deprecated = "retired in PS3.16"]
pub const IntraductalCarcinomaMicroPapillary: Code<'static> = c("111314", "Intraductal carcinoma micro-papillary");
pub const IntraductalComedocarcinomaWithNecrosis: Code<'static> = c("111312", "Intraductal comedocarcinoma with necrosis");
pub const IntralobularLines: Code<'static> = c("112108", "Intralobular lines");
pub const IntraluminalFillingDefect: Code<'static> = c("111425", "Intraluminal filling defect");
pub const IntraocularLensCalculation: Code<'static> = c("IOL", "Intraocular Lens Calculation");
pub const IntraoperativeFixedAperture: Code<'static> = c("130345", "Intraoperative Fixed Aperture");
pub const IntraoperativeTransducer: Code<'static> = c("125267", "Intraoperative Transducer");
pub const IntravascularOpticalCoherenceTomography: Code<'static> = c("IVOCT", "Intravascular Optical Coherence Tomography");
pub const IntravascularTransducer: Code<'static> = c("125265", "Intravascular Transducer");
pub const IntravascularUltrasound: Code<'static> = c("IVUS", "Intravascular Ultrasound");
pub const IntravenousExtravasationSymptoms: Code<'static> = c("113505", "Intravenous Extravasation Symptoms");
pub const InvasiveAndInSituCarcinoma: Code<'static> = c("111316", "Invasive and in-situ carcinoma");
pub const InvasiveBreastCarcinoma: Code<'static> = c("111389", "Invasive breast carcinoma");
#[deprecated = "retired in PS3.16"]
pub const InvasiveCribriformCarcinoma: Code<'static> = c("111342", "Invasive cribriform carcinoma");
#[deprecated = "retired in PS3.16"]
pub const InvasiveLobularCarcinoma: Code<'static> = c("111317", "Invasive lobular carcinoma");
#[deprecated = "retired in PS3.16"]
pub const InverseDifferenceMomentNormalizedOfGLCM: Code<'static> = c("128792", "Inverse Difference Moment Normalized of GLCM");
#[deprecated = "retired in PS3.16"]
pub const InverseDifferenceMomentOfGLCM: Code<'static> = c("126062", "Inverse Difference Moment of GLCM");
#[deprecated = "retired in PS3.16"]
pub const InverseDifferenceNormalizedOfGLCM: Code<'static> = c("128791", "Inverse Difference Normalized of GLCM");
#[deprecated = "retired in PS3.16"]
pub const InverseDifferenceOfGLCM: Code<'static> = c("128790", "Inverse Difference of GLCM");
#[deprecated = "retired in PS3.16"]
pub const InverseVarianceOfGLCM: Code<'static> = c("128793", "Inverse Variance of GLCM");
pub const InvestigationalNewDrug: Code<'static> = c("113530", "Investigational New Drug");
pub const Involved: Code<'static> = c("111471", "Involved");
pub const IodinatedI125DPA713: Code<'static> = c("126764", "Iodinated I^125^ DPA-713");
pub const IonChamber: Code<'static> = c("128707", "Ion Chamber");
pub const IonChamberArray: Code<'static> = c("128703", "Ion Chamber Array");
pub const IonTherapyParticle: Code<'static> = c("130037", "Ion Therapy Particle");
pub const IrradiatingDevice: Code<'static> = c("113859", "Irradiating Device");
pub const IrradiationAdministering: Code<'static> = c("113851", "Irradiation Administering");
pub const IrradiationAuthorizing: Code<'static> = c("113850", "Irradiation Authorizing");
pub const IrradiationDetails: Code<'static> = c("130505", "Irradiation Details");
pub const IrradiationDuration: Code<'static> = c("113742", "Irradiation Duration");
pub const IrradiationEvent: Code<'static> = c("113852", "Irradiation Event");
pub const IrradiationEventLabel: Code<'static> = c("113605", "Irradiation Event Label");
pub const IrradiationEventSummaryData: Code<'static> = c("130501", "Irradiation Event Summary Data");
pub const IrradiationEventType: Code<'static> = c("113721", "Irradiation Event Type");
pub const IrradiationEventUID: Code<'static> = c("113769", "Irradiation Event UID");
#[deprecated = "retired in PS3.16"]
pub const IrradiationEventUID_113853: Code<'static> = c("113853", "Irradiation Event UID");
pub const IrradiationEventXRayData: Code<'static> = c("113706", "Irradiation Event X-Ray Data");
pub const IsIonic: Code<'static> = c("130189", "Is Ionic");
pub const IsRejectedAcquisition: Code<'static> = c("130503", "Is Rejected Acquisition");
pub const IsRepeatedAcquisition: Code<'static> = c("128551", "Is Repeated Acquisition");
pub const IsocenterOrigin: Code<'static> = c("130539", "Isocenter Origin");
pub const IsocentricPatientSupportContinuousPitchAngle: Code<'static> = c("126812", "Isocentric Patient Support Continuous Pitch Angle");
pub const IsocentricPatientSupportContinuousRollAngle: Code<'static> = c("126813", "Isocentric Patient Support Continuous Roll Angle");
pub const IsocentricPatientSupportContinuousYawAngle: Code<'static> = c("126814", "Isocentric Patient Support Continuous Yaw Angle");
pub const IsocentricPatientSupportLateralPosition: Code<'static> = c("126815", "Isocentric Patient Support Lateral Position");
pub const IsocentricPatientSupportLongitudinalPosition: Code<'static> = c("126816", "Isocentric Patient Support Longitudinal Position");
pub const IsocentricPatientSupportVerticalPosition: Code<'static> = c("126817", "Isocentric Patient Support Vertical Position");
pub const IsocentricSetupMethod: Code<'static> = c("130630", "Isocentric Setup Method");
pub const IsocentricTreatmentLocationPoint: Code<'static> = c("130073", "Isocentric Treatment Location Point");
pub const Isodose: Code<'static> = c("128484", "Isodose");
pub const IsodoseVolume: Code<'static> = c("130747", "Isodose Volume");
pub const Isoechoic: Code<'static> = c("111365", "Isoechoic");
pub const IssuerOfIdentifier: Code<'static> = c("110190", "Issuer of Identifier");
pub const IssuerOfParentSpecimenIdentifier: Code<'static> = c("111706", "Issuer of Parent Specimen Identifier");
pub const IssuerOfSpecimenIdentifier: Code<'static> = c("111724", "Issuer of Specimen Identifier");
pub const IterativeClosestPoint: Code<'static> = c("114213", "Iterative Closest Point");
pub const IterativeMAR: Code<'static> = c("131540", "Iterative MAR");
pub const IterativeReconstruction: Code<'static> = c("113963", "Iterative Reconstruction");
pub const J59189Zr: Code<'static> = c("126514", "J591 ^89^Zr");
pub const JDMSFetalAnatomy2014: Code<'static> = c("131384", "JDMS Fetal Anatomy 2014");
pub const JSUMFetalMorphology2021: Code<'static> = c("131383", "JSUM Fetal Morphology 2021");
#[deprecated = "retired in PS3.16"]
pub const JWire: Code<'static> = c("111170", "J Wire");
pub const JaccardIndex: Code<'static> = c("130921", "Jaccard Index");
pub const JawPair: Code<'static> = c("130330", "Jaw Pair");
pub const JawPositionToleranceViolation: Code<'static> = c("130471", "Jaw Position Tolerance Violation");
#[deprecated = "retired in PS3.16"]
pub const JointAverageOfGLCM: Code<'static> = c("128782", "Joint Average of GLCM");
#[deprecated = "retired in PS3.16"]
pub const JointEntropyOfGLCM: Code<'static> = c("126060", "Joint Entropy of GLCM");
#[deprecated = "retired in PS3.16"]
pub const JointMaximumOfGLCM: Code<'static> = c("128781", "Joint Maximum of GLCM");
pub const JointPositionMethod: Code<'static> = c("109132", "Joint position method");
#[deprecated = "retired in PS3.16"]
pub const JointVarianceOfGLCM: Code<'static> = c("128783", "Joint Variance of GLCM");
pub const JugularLymphaticSac: Code<'static> = c("131375", "Jugular lymphatic sac");
pub const JuvenilePapillomatosis: Code<'static> = c("111277", "Juvenile papillomatosis");
pub const KTypeFilter: Code<'static> = c("130765", "K-type filter");
pub const KVP: Code<'static> = c("113733", "KVP");
pub const Kane: Code<'static> = c("111868", "Kane");
pub const KaneKeratoconus: Code<'static> = c("111870", "Kane Keratoconus");
pub const KaneToric: Code<'static> = c("111869", "Kane Toric");
pub const KeepVeinOpenEnded: Code<'static> = c("130162", "Keep vein open ended");
pub const KeepVeinOpenStarted: Code<'static> = c("130161", "Keep vein open started");
pub const Kep: Code<'static> = c("126313", "kep");
pub const Keratometry: Code<'static> = c("KER", "Keratometry");
pub const KeratometryMeasurementsSOPInstance: Code<'static> = c("111757", "Keratometry Measurements SOP Instance");
pub const KerleyALine: Code<'static> = c("112109", "Kerley A line");
pub const KerleyBLine: Code<'static> = c("112110", "Kerley B line");
pub const KerleyCLines: Code<'static> = c("112111", "Kerley C lines");
#[deprecated = "retired in PS3.16"]
pub const KeyImages: Code<'static> = c("121180", "Key Images");
pub const KeyObjectDescription: Code<'static> = c("113012", "Key Object Description");
pub const KeyObjectSelection: Code<'static> = c("KO", "Key Object Selection");
pub const KidneyStent: Code<'static> = c("112175", "Kidney stent");
pub const KnownBiopsyProvenMalignancy: Code<'static> = c("111125", "Known biopsy proven malignancy");
pub const KnownBiopsyProvenMalignancyTakeAppropriateAction: Code<'static> = c("111122", "Known biopsy proven malignancy - take appropriate action");
pub const Krishnan2016: Code<'static> = c("131534", "Krishnan 2016");
pub const Ktrans: Code<'static> = c("126312", "Ktrans");
pub const Kurtosis: Code<'static> = c("126052", "Kurtosis");
pub const KurtosisDiffusionCoefficient: Code<'static> = c("113294", "Kurtosis Diffusion Coefficient");
pub const KurtosisDiffusionModel: Code<'static> = c("113252", "Kurtosis diffusion model");
pub const LADRegionInLAOProjection: Code<'static> = c("122470", "LAD Region in LAO Projection");
pub const LADRegionInRAOProjection: Code<'static> = c("122464", "LAD Region in RAO Projection");
pub const LBWOrIUGR: Code<'static> = c("111593", "LBW or IUGR");
pub const LDLCholesterolScoreSheetForMen: Code<'static> = c("122233", "LDL Cholesterol Score Sheet for Men");
pub const LDLCholesterolScoreSheetForWomen: Code<'static> = c("122234", "LDL Cholesterol Score Sheet for Women");
pub const LSDI: Code<'static> = c("113225", "LSDI");
pub const LVEndSystolicGLS2CHEndocardial: Code<'static> = c("130688", "LV End-systolic GLS 2CH (Endocardial)");
pub const LVEndSystolicGLS4CHEndocardial: Code<'static> = c("130687", "LV End-systolic GLS 4CH (Endocardial)");
pub const LVEndSystolicGLSALAXEndocardial: Code<'static> = c("130689", "LV End-systolic GLS ALAX (Endocardial)");
pub const LVEndSystolicGLSMultiviewMeanEndocardial: Code<'static> = c("130686", "LV End-systolic GLS Multiview Mean (Endocardial)");
pub const LVEndSystolicMechanicalDispersion: Code<'static> = c("130698", "LV End-systolic Mechanical Dispersion");
pub const LVPeakGLS2CHEndocardial: Code<'static> = c("130696", "LV Peak GLS 2CH (Endocardial)");
pub const LVPeakGLS4CHEndocardial: Code<'static> = c("130695", "LV Peak GLS 4CH (Endocardial)");
pub const LVPeakGLSALAXEndocardial: Code<'static> = c("130697", "LV Peak GLS ALAX (Endocardial)");
pub const LVPeakGLSMultiviewMeanEndocardial: Code<'static> = c("130694", "LV Peak GLS Multiview Mean (Endocardial)");
pub const LVPeakSystolicGLS2CHEndocardial: Code<'static> = c("130692", "LV Peak Systolic GLS 2CH (Endocardial)");
pub const LVPeakSystolicGLS4CHEndocardial: Code<'static> = c("130691", "LV Peak Systolic GLS 4CH (Endocardial)");
pub const LVPeakSystolicGLSALAXEndocardial: Code<'static> = c("130693", "LV Peak Systolic GLS ALAX (Endocardial)");
pub const LVPeakSystolicGLSMultiviewMeanEndocardial: Code<'static> = c("130690", "LV Peak Systolic GLS Multiview Mean (Endocardial)");
pub const LVWallMotionScoreIndex: Code<'static> = c("125202", "LV Wall Motion Score Index");
pub const LWHMethodForVolumeOfEllipsoid: Code<'static> = c("126029", "LWH method for volume of ellipsoid");
pub const LabelType: Code<'static> = c("113606", "Label Type");
#[deprecated = "retired in PS3.16"]
pub const LactatingAdenoma: Code<'static> = c("111278", "Lactating adenoma");
pub const LactationalChange: Code<'static> = c("111279", "Lactational change");
pub const LandmarkBasedWallMotionAnalysis: Code<'static> = c("122497", "Landmark Based Wall Motion Analysis");
pub const Language: Code<'static> = c("121045", "Language");
pub const LanguageOfContentItemAndDescendants: Code<'static> = c("121049", "Language of Content Item and Descendants");
pub const LanguageOfNameAndValue: Code<'static> = c("121048", "Language of Name and Value");
pub const LanguageOfValue: Code<'static> = c("121047", "Language of Value");
#[deprecated = "retired in PS3.16"]
pub const Laparoscopy: Code<'static> = c("LP", "Laparoscopy");
pub const LargeDuctPapilloma: Code<'static> = c("111281", "Large duct papilloma");
pub const LargeGreaterThan380CmLateralThickness: Code<'static> = c("113603", "Large: > 38.0 cm lateral thickness");
#[deprecated = "retired in PS3.16"]
pub const LargeZoneEmphasis: Code<'static> = c("128822", "Large Zone Emphasis");
#[deprecated = "retired in PS3.16"]
pub const LargeZoneHighGrayLevelEmphasis: Code<'static> = c("128828", "Large Zone High Gray Level Emphasis");
#[deprecated = "retired in PS3.16"]
pub const LargeZoneLowGrayLevelEmphasis: Code<'static> = c("128827", "Large Zone Low Gray Level Emphasis");
pub const LaserCrossHairs: Code<'static> = c("128151", "Laser Cross-hairs");
pub const LaserScanning: Code<'static> = c("114203", "Laser scanning");
pub const LaserScanning3DCAMModel: Code<'static> = c("129021", "Laser Scanning 3D CAM model");
pub const LaserSurfaceScan: Code<'static> = c("LS", "Laser surface scan");
pub const LastChordOfAbnormalRegion: Code<'static> = c("122453", "Last Chord of Abnormal Region");
pub const LateChildBearingAfter30: Code<'static> = c("111555", "Late child bearing (after 30)");
pub const LateContrastEnhancement: Code<'static> = c("122664", "Late Contrast Enhancement");
pub const LeadID: Code<'static> = c("122148", "Lead ID");
pub const LeadRadiologicTechnologist: Code<'static> = c("128674", "Lead Radiologic Technologist");
pub const LeafPairs: Code<'static> = c("130331", "Leaf Pairs");
pub const LeastMeanSquareDeconvolution: Code<'static> = c("126310", "Least Mean Square deconvolution");
pub const LeastSquaresFitOfMultipleSamples: Code<'static> = c("113261", "Least squares fit of multiple samples");
pub const LeftArmRestPitchAngle: Code<'static> = c("130846", "Left Arm Rest Pitch Angle");
pub const LeftArmRestPosition: Code<'static> = c("130845", "Left Arm Rest Position");
pub const LeftArmRestRollAngle: Code<'static> = c("130847", "Left Arm Rest Roll Angle");
pub const LeftAtrialAppendageClosureDevice: Code<'static> = c("131112", "Left atrial appendage closure device");
pub const LeftAtrialAppendageClosureDeviceCircumference: Code<'static> = c("131217", "Left atrial appendage closure device circumference");
pub const LeftAtrialAppendageClosureDeviceCompressionRatio: Code<'static> = c("131218", "Left atrial appendage closure device compression ratio");
pub const LeftAtrialAppendageClosureDeviceDiameter: Code<'static> = c("131219", "Left atrial appendage closure device diameter");
pub const LeftAtrialAppendageClosureDeviceSize: Code<'static> = c("131220", "Left atrial appendage closure device size");
pub const LeftAtrialAppendageDepth: Code<'static> = c("131221", "Left atrial appendage depth");
pub const LeftAtrialAppendageLandingZoneMajorAxis: Code<'static> = c("131222", "Left atrial appendage landing zone major axis");
pub const LeftAtrialAppendageMajorAxis: Code<'static> = c("131223", "Left atrial appendage major axis");
pub const LeftAtrialAppendageMinorAxis: Code<'static> = c("131224", "Left atrial appendage minor axis");
pub const LeftAtrialAppendageOstiumPerimeter: Code<'static> = c("131225", "Left atrial appendage ostium perimeter");
pub const LeftAtrialEDVolume: Code<'static> = c("122407", "Left Atrial ED Volume");
pub const LeftAtrialESVolume: Code<'static> = c("122408", "Left Atrial ES Volume");
pub const LeftAtrialEjectionFractionByAngiography: Code<'static> = c("122406", "Left Atrial Ejection Fraction by Angiography");
pub const LeftAtriumDescendingAortaDistance: Code<'static> = c("131003", "Left Atrium-Descending Aorta Distance");
pub const LeftContour: Code<'static> = c("122507", "Left Contour");
pub const LeftEyeRx: Code<'static> = c("111689", "Left Eye Rx");
pub const LeftFirst: Code<'static> = c("126830", "left first");
pub const LeftHemisphereMostAnterior: Code<'static> = c("125035", "Left Hemisphere Most Anterior");
pub const LeftHemisphereMostInferior: Code<'static> = c("125038", "Left Hemisphere Most Inferior");
pub const LeftHemisphereMostPosterior: Code<'static> = c("125036", "Left Hemisphere Most Posterior");
pub const LeftHemisphereMostSuperior: Code<'static> = c("125037", "Left Hemisphere Most Superior");
pub const LeftMainCoronaryOstiumHeight: Code<'static> = c("131166", "Left main coronary ostium height");
pub const LeftRightAxis: Code<'static> = c("110860", "Left-Right Axis");
pub const LeftToRight: Code<'static> = c("110867", "Left To Right");
pub const LeftToRightFlow: Code<'static> = c("122227", "Left to Right Flow");
pub const LeftVentricleApicalAnterolateralSegment: Code<'static> = c("130623", "left ventricle apical anterolateral segment");
pub const LeftVentricleApicalAnteroseptalSegment: Code<'static> = c("130620", "left ventricle apical anteroseptal segment");
pub const LeftVentricleApicalInferolateralSegment: Code<'static> = c("130622", "left ventricle apical inferolateral segment");
pub const LeftVentricleApicalInferoseptalSegment: Code<'static> = c("130621", "left ventricle apical inferoseptal segment");
pub const LeftVentricleMassByAreaLength: Code<'static> = c("125270", "Left Ventricle Mass by Area Length");
pub const LeftVentricleMassByAreaLengthAdjustedByHeight: Code<'static> = c("125273", "Left Ventricle Mass by Area Length - adjusted by Height");
pub const LeftVentricleMassByMMode: Code<'static> = c("125221", "Left Ventricle Mass by M-mode");
pub const LeftVentricleMassByMModeAdjustedByHeight: Code<'static> = c("125271", "Left Ventricle Mass by M-mode - adjusted by Height");
pub const LeftVentricleMassByTruncatedEllipse: Code<'static> = c("125222", "Left Ventricle Mass by Truncated Ellipse");
pub const LeftVentricleMassByTruncatedEllipseAdjustedByHeight: Code<'static> = c("125272", "Left Ventricle Mass by Truncated Ellipse - adjusted by Height");
pub const LeftVentricularEjectionTime: Code<'static> = c("122211", "Left Ventricular ejection time");
pub const LeftVentricularFillingTime: Code<'static> = c("122212", "Left Ventricular filling time");
pub const LeftVentricularInletLength: Code<'static> = c("131018", "Left ventricular inlet length");
pub const LeftVentricularOutflowTractCalcificationSeverity: Code<'static> = c("131167", "Left ventricular outflow tract calcification severity");
pub const LeftVentricularOutflowTractView: Code<'static> = c("131028", "Left ventricular outflow tract view");
pub const LeftVentricularPosterobasalSegment: Code<'static> = c("122582", "Left ventricular posterobasal segment");
pub const LengthLuminalSegment: Code<'static> = c("122510", "Length Luminal Segment");
pub const LengthOfReconstructableVolume: Code<'static> = c("113893", "Length of Reconstructable Volume");
pub const Lensometry: Code<'static> = c("LEN", "Lensometry");
pub const LesionBoundary: Code<'static> = c("111357", "Lesion boundary");
pub const LesionCapsularContactLength: Code<'static> = c("130558", "Lesion capsular contact length");
pub const LesionCentered: Code<'static> = c("111902", "Lesion centered");
pub const LesionCompletelyRemoved: Code<'static> = c("111447", "Lesion completely removed");
pub const LesionDensity: Code<'static> = c("111035", "Lesion Density");
pub const LesionIdentifier: Code<'static> = c("121151", "Lesion Identifier");
pub const LesionInterventionInformation: Code<'static> = c("122178", "Lesion Intervention Information");
pub const LesionMorphology: Code<'static> = c("122133", "Lesion Morphology");
pub const LesionPartiallyRemoved: Code<'static> = c("111448", "Lesion partially removed");
pub const LesionRisk: Code<'static> = c("121153", "Lesion Risk");
pub const LesionSensitivity: Code<'static> = c("111089", "Lesion Sensitivity");
pub const LesionToBackgroundSUVRatio: Code<'static> = c("126039", "Lesion to Background SUV Ratio");
pub const LessThan3MonthsAgo: Code<'static> = c("111396", "< 3 months ago");
pub const LeukemicInfiltration: Code<'static> = c("111318", "Leukemic infiltration");
pub const LevelOfDifficulty: Code<'static> = c("128724", "Level of Difficulty");
pub const LevelOfSignificance: Code<'static> = c("121403", "Level of Significance");
pub const LevenbergMarquardt: Code<'static> = c("113265", "Levenberg-Marquardt");
pub const Levovist: Code<'static> = c("125904", "Levovist");
pub const LightBrownColor: Code<'static> = c("111450", "Light brown color");
pub const LightsOnTimeOfDay: Code<'static> = c("127215", "Lights on time of day");
pub const Line: Code<'static> = c("112083", "Line");
pub const LineBScanPattern: Code<'static> = c("128281", "Line B-scan pattern");
pub const LineNoiseArtifact: Code<'static> = c("130886", "Line noise artifact");
pub const LineScanPattern: Code<'static> = c("125240", "Line scan pattern");
pub const LineSegmentLength: Code<'static> = c("121227", "Line segment length");
pub const Linear: Code<'static> = c("112150", "Linear");
pub const LinearBoltzmannTransportEquation: Code<'static> = c("131320", "Linear Boltzmann Transport Equation");
pub const LinearCellKillFactor: Code<'static> = c("130088", "Linear Cell Kill Factor");
pub const LinearCurve: Code<'static> = c("130253", "Linear Curve");
pub const LinearDisplacement: Code<'static> = c("110856", "Linear Displacement");
pub const LinearQuadraticLinearModel: Code<'static> = c("130133", "Linear-Quadratic-Linear Model");
pub const LinearQuadraticModel: Code<'static> = c("130131", "Linear-Quadratic Model");
pub const LinearQuadraticModelForLowDoseRateBrachytherapy: Code<'static> = c("130134", "Linear-Quadratic Model for Low-Dose Rate Brachytherapy");
pub const LinearQuadraticModelWithTimeFactor: Code<'static> = c("130132", "Linear-Quadratic Model with Time Factor");
pub const LinearSpiculation: Code<'static> = c("113665", "Linear spiculation");
pub const LinearUltrasoundTransducerGeometry: Code<'static> = c("125252", "Linear ultrasound transducer geometry");
pub const LipidAndLactate: Code<'static> = c("113095", "Lipid and Lactate");
pub const LiquidCrystalDisplay: Code<'static> = c("109992", "Liquid Crystal Display");
pub const LiquidIonChamber: Code<'static> = c("128705", "Liquid Ion Chamber");
pub const Lobar: Code<'static> = c("112158", "Lobar");
pub const Lobulated: Code<'static> = c("112135", "Lobulated");
pub const LocalEffectModel: Code<'static> = c("130128", "Local Effect Model");
pub const LocalServiceOperationStarted: Code<'static> = c("110141", "Local Service Operation Started");
pub const LocalServiceOperationStopped: Code<'static> = c("110142", "Local Service Operation Stopped");
pub const LocalizedProstateFinding: Code<'static> = c("130555", "Localized Prostate Finding");
pub const Localizer: Code<'static> = c("121311", "Localizer");
pub const LocallyManufacturedProduct: Code<'static> = c("127390", "Locally manufactured product");
pub const LocationInChest: Code<'static> = c("112013", "Location in Chest");
pub const LocationOfSamplingSite: Code<'static> = c("111709", "Location of sampling site");
pub const LocationOfSamplingSiteXOffset: Code<'static> = c("111710", "Location of sampling site X offset");
pub const LocationOfSamplingSiteYOffset: Code<'static> = c("111711", "Location of sampling site Y offset");
pub const LocationOfSamplingSiteZOffset: Code<'static> = c("111712", "Location of sampling site Z offset");
pub const LocationOfSpecimen: Code<'static> = c("111718", "Location of Specimen");
pub const LocationOfSpecimenXOffset: Code<'static> = c("111719", "Location of Specimen X offset");
pub const LocationOfSpecimenYOffset: Code<'static> = c("111720", "Location of Specimen Y offset");
pub const LocationOfSpecimenZOffset: Code<'static> = c("111721", "Location of Specimen Z offset");
pub const LogOfRatioOfTwoSamples: Code<'static> = c("113260", "Log of ratio of two samples");
pub const Login: Code<'static> = c("110122", "Login");
pub const Logout: Code<'static> = c("110123", "Logout");
pub const LongAxisBased: Code<'static> = c("122476", "Long Axis Based");
#[deprecated = "retired in PS3.16"]
pub const LongRunHighGrayLevelEmphasis: Code<'static> = c("128808", "Long Run High Gray Level Emphasis");
#[deprecated = "retired in PS3.16"]
pub const LongRunLowGrayLevelEmphasis: Code<'static> = c("128807", "Long Run Low Gray Level Emphasis");
#[deprecated = "retired in PS3.16"]
pub const LongRunsEmphasis: Code<'static> = c("128802", "Long Runs Emphasis");
pub const LongitudinalLagrangianStrain: Code<'static> = c("130670", "Longitudinal Lagrangian Strain");
pub const LongitudinalLagrangianStrainRate: Code<'static> = c("130674", "Longitudinal Lagrangian Strain Rate");
pub const LongitudinalModulation: Code<'static> = c("114102", "Longitudinal modulation");
pub const LongitudinalPositionZ: Code<'static> = c("113994", "Longitudinal Position Z");
pub const LongitudinalTemporalEventType: Code<'static> = c("128741", "Longitudinal Temporal Event Type");
pub const LongitudinalTemporalOffsetFromEvent: Code<'static> = c("128740", "Longitudinal Temporal Offset from Event");
pub const LossyCompressedImage: Code<'static> = c("121325", "Lossy compressed image");
pub const LossyCompressedPredecessor: Code<'static> = c("121330", "Lossy compressed predecessor");
pub const LossyCompression: Code<'static> = c("113040", "Lossy Compression");
pub const LotIdentifier: Code<'static> = c("121149", "Lot Identifier");
pub const Low: Code<'static> = c("111577", "Low");
#[deprecated = "retired in PS3.16"]
pub const LowGrayLevelRunEmphasis: Code<'static> = c("128803", "Low Gray Level Run Emphasis");
#[deprecated = "retired in PS3.16"]
pub const LowGrayLevelZoneEmphasis: Code<'static> = c("128823", "Low Gray Level Zone Emphasis");
pub const LowThyroidUptake: Code<'static> = c("113561", "Low Thyroid Uptake");
pub const Lucency: Code<'static> = c("112084", "Lucency");
pub const LumenDiameterRatio: Code<'static> = c("122350", "Lumen Diameter Ratio");
pub const LumenEccentricityIndex: Code<'static> = c("122343", "Lumen Eccentricity Index");
pub const LumenPerimeter: Code<'static> = c("122332", "Lumen Perimeter");
pub const LumenShapeIndex: Code<'static> = c("122348", "Lumen Shape Index");
pub const LumenVolume: Code<'static> = c("122372", "Lumen Volume");
pub const LuminanceResponseEvaluation: Code<'static> = c("109703", "Luminance response evaluation");
pub const LuminanceUniformityEvaluation: Code<'static> = c("109704", "Luminance uniformity evaluation");
pub const Lutetium177NAcetylaspartylglutamate: Code<'static> = c("126509", "Lutetium^177^ n-acetylaspartylglutamate");
pub const LympathicVesselInvasion: Code<'static> = c("111320", "Lympathic vessel invasion");
#[deprecated = "retired in PS3.16"]
pub const Lymphoma: Code<'static> = c("111321", "Lymphoma");
pub const MDerivedFilter: Code<'static> = c("130766", "M-derived filter");
pub const MEDI55189Zr: Code<'static> = c("126730", "MEDI-551 ^89^Zr");
pub const MEMSBasedTransducer: Code<'static> = c("130816", "MEMS-based Transducer");
pub const MIRDOSE: Code<'static> = c("113526", "MIRDOSE");
pub const MIRDPamphlet1: Code<'static> = c("113520", "MIRD Pamphlet 1");
pub const MLCPositionInterlock: Code<'static> = c("130829", "MLC Position Interlock");
pub const MLCPositionToleranceViolation: Code<'static> = c("130472", "MLC Position Tolerance Violation");
pub const MLOEvidenceOfMotionBlur: Code<'static> = c("111200", "MLO Evidence of motion blur");
pub const MLOInframammaryFoldIsNotOpen: Code<'static> = c("111201", "MLO Inframammary fold is not open");
pub const MLOInsufficientPectoralMuscle: Code<'static> = c("111197", "MLO Insufficient pectoral muscle");
pub const MLONoFatIsVisualizedPosteriorToFibroglandularTissues: Code<'static> = c("111198", "MLO No fat is visualized posterior to fibroglandular tissues");
pub const MLOPoorSeparationOfDeepAndSuperficialBreastTissues: Code<'static> = c("111199", "MLO Poor separation of deep and superficial breast tissues");
pub const MLlerMethodPlanningForHipReplacement: Code<'static> = c("112344", "Müller Method Planning for Hip Replacement");
pub const MODImported: Code<'static> = c("110026", "MOD Imported");
pub const MPPSContent: Code<'static> = c("113858", "MPPS Content");
pub const MRImageReconstruction: Code<'static> = c("110016", "MR Image Reconstruction");
pub const MRMarker: Code<'static> = c("129308", "MR Marker");
pub const MRPerfusionPeak: Code<'static> = c("122635", "MR Perfusion Peak");
pub const MRPerfusionSlope: Code<'static> = c("122636", "MR Perfusion Slope");
pub const MRPerfusionTimeIntegral: Code<'static> = c("122637", "MR Perfusion Time Integral");
pub const MRSignalIntensity: Code<'static> = c("110852", "MR signal intensity");
pub const MVAEqualsFlowPer380SqrtGradientMmHg: Code<'static> = c("122263", "MVA = Flow / 38.0 * sqrt(Gradient[mmHg])");
pub const MachineCalibrationAdjustment: Code<'static> = c("130465", "Machine Calibration Adjustment");
pub const MachineCapabilityLicenseExpired: Code<'static> = c("130467", "Machine Capability License Expired");
pub const MachineClearanceInterlock: Code<'static> = c("130740", "Machine Clearance Interlock");
pub const MachineNotAvailable: Code<'static> = c("130463", "Machine Not Available");
pub const MachineQualityAssuranceInputUsed: Code<'static> = c("128216", "Machine Quality Assurance Input Used");
pub const Macrocalcifications: Code<'static> = c("111345", "Macrocalcifications");
pub const MaculaCentered: Code<'static> = c("111900", "Macula centered");
pub const MacularFixationTesting: Code<'static> = c("111845", "Macular Fixation Testing");
pub const MacularGridThicknessAndVolumeReport: Code<'static> = c("111690", "Macular Grid Thickness and Volume Report");
pub const MacularThicknessKeyMeasurements: Code<'static> = c("131243", "Macular Thickness Key Measurements");
pub const MagneticFieldStrength: Code<'static> = c("130542", "Magnetic field strength");
pub const MagneticResonance: Code<'static> = c("MR", "Magnetic Resonance");
#[deprecated = "retired in PS3.16"]
pub const MagneticResonanceAngiography: Code<'static> = c("MA", "Magnetic resonance angiography");
#[deprecated = "retired in PS3.16"]
pub const MagneticResonanceSpectroscopy: Code<'static> = c("MS", "Magnetic resonance spectroscopy");
pub const MagneticSusceptibility: Code<'static> = c("126396", "Magnetic Susceptibility");
pub const MagnetizationTransferRatio: Code<'static> = c("113098", "Magnetization Transfer Ratio");
pub const MagnificationSelection: Code<'static> = c("112709", "Magnification selection");
pub const MagnitudeNormalized: Code<'static> = c("130776", "Magnitude normalized");
pub const Male: Code<'static> = c("M", "Male");
pub const MaleChangedToFemale: Code<'static> = c("MC", "Male changed to Female");
pub const MalePseudohermaphrodite: Code<'static> = c("MP", "Male Pseudohermaphrodite");
pub const MaleTypical: Code<'static> = c("131231", "Male-typical");
pub const MalignancyType: Code<'static> = c("111388", "Malignancy Type");
#[deprecated = "retired in PS3.16"]
pub const MalignantFibrousHistiocytoma: Code<'static> = c("111331", "Malignant fibrous histiocytoma");
pub const MalignantMelanomaOfNipple: Code<'static> = c("111334", "Malignant melanoma of nipple");
pub const MammographicCrosshair: Code<'static> = c("111487", "Mammographic (crosshair)");
pub const MammographicGrid: Code<'static> = c("111488", "Mammographic (grid)");
pub const Mammography: Code<'static> = c("MG", "Mammography");
pub const MammographyCAD: Code<'static> = c("111411", "Mammography CAD");
pub const MammographyCADReport: Code<'static> = c("111036", "Mammography CAD Report");
pub const MammographyQualityControlManual1999ACR: Code<'static> = c("111238", "Mammography Quality Control Manual 1999, ACR");
pub const Manifest: Code<'static> = c("113030", "Manifest");
pub const ManifestWithDescription: Code<'static> = c("131560", "Manifest with Description");
pub const ManualAdministration: Code<'static> = c("130174", "Manual Administration");
pub const ManualEntry: Code<'static> = c("113857", "Manual Entry");
pub const ManualHandlingInterval: Code<'static> = c("127152", "Manual handling interval");
pub const ManualHoldStarted: Code<'static> = c("130267", "Manual hold started");
pub const ManualKeratometry: Code<'static> = c("111753", "Manual Keratometry");
pub const ManualProcessing: Code<'static> = c("123109", "Manual Processing");
pub const ManualResumeFromHold: Code<'static> = c("130268", "Manual resume from hold");
pub const ManuallyTriggeredInjectionInformation: Code<'static> = c("130172", "Manually Triggered Injection Information");
pub const ManufacturerImplantTemplate: Code<'static> = c("112371", "Manufacturer Implant Template");
pub const Margetuximab89Zr: Code<'static> = c("126740", "Margetuximab ^89^Zr");
#[deprecated = "retired in PS3.16"]
pub const Margins: Code<'static> = c("111037", "Margins");
pub const MarkerPlacement: Code<'static> = c("111123", "Marker placement");
pub const MaskImageForImageProcessingOperation: Code<'static> = c("121321", "Mask image for image processing operation");
pub const MassAdministered: Code<'static> = c("121383", "Mass administered");
pub const MassInTheSkin: Code<'static> = c("111112", "Mass in the skin");
pub const MassOccupancyImage: Code<'static> = c("129329", "Mass Occupancy Image");
pub const MassOfBreast: Code<'static> = c("112195", "Mass of breast");
pub const MassOfParenchymalTissue: Code<'static> = c("112194", "Mass of parenchymal tissue");
pub const MassOfVascularCalcification: Code<'static> = c("112199", "Mass of Vascular Calcification");
pub const MassOnTheSkin: Code<'static> = c("111113", "Mass on the skin");
pub const MassScoringMethod: Code<'static> = c("112057", "Mass scoring method");
pub const MassWithCalcifications: Code<'static> = c("111459", "Mass with calcifications");
pub const MaterialHighlightedImage: Code<'static> = c("129325", "Material Highlighted Image");
pub const MaterialRecalculatedImage: Code<'static> = c("129327", "Material Recalculated Image");
pub const MaterialRemovedImage: Code<'static> = c("129324", "Material Removed Image");
pub const MaterialSpecificImage: Code<'static> = c("129323", "Material Specific Image");
pub const MaterialSuppressedImage: Code<'static> = c("129326", "Material Suppressed Image");
pub const Maternal: Code<'static> = c("111541", "Maternal");
pub const MatingFeatureID: Code<'static> = c("112352", "Mating Feature ID");
pub const MatingFeatureSetID: Code<'static> = c("112351", "Mating Feature Set ID");
pub const MaxDpPerDt: Code<'static> = c("109025", "Max dp/dt");
pub const MaxDpPerDtPerP: Code<'static> = c("122190", "Max dp/dt/P");
pub const MaxNegDpPerDt: Code<'static> = c("109026", "Max neg dp/dt");
pub const MaxVolumeNormalizedTo50mmHgPulsePressure: Code<'static> = c("122238", "Max volume normalized to 50mmHg pulse pressure");
pub const MaximumAbsoluteVolumeAtRadiationDose: Code<'static> = c("130017", "Maximum Absolute Volume at Radiation Dose");
pub const MaximumAbsorbedRadiationDose: Code<'static> = c("128531", "Maximum Absorbed Radiation Dose");
pub const MaximumAorticPlaqueThickness: Code<'static> = c("131168", "Maximum aortic plaque thickness");
pub const MaximumAttenuationCoefficient: Code<'static> = c("112180", "Maximum Attenuation Coefficient");
pub const MaximumCADOperatingPoint: Code<'static> = c("111072", "Maximum CAD Operating Point");
pub const MaximumDifference: Code<'static> = c("126376", "Maximum Difference");
pub const MaximumEquivalentRadiationDose: Code<'static> = c("128535", "Maximum Equivalent Radiation Dose");
pub const MaximumEquivalentUniformDose: Code<'static> = c("130008", "Maximum Equivalent Uniform Dose");
pub const MaximumHomogeneityIndex: Code<'static> = c("130013", "Maximum Homogeneity Index");
pub const MaximumIntensityProjection: Code<'static> = c("113078", "Maximum intensity projection");
pub const MaximumMeanRadiationDose: Code<'static> = c("130006", "Maximum Mean Radiation Dose");
pub const MaximumOrificeArea: Code<'static> = c("125328", "Maximum Orifice Area");
pub const MaximumPercentVolumeAtRadiationDose: Code<'static> = c("130015", "Maximum Percent Volume at Radiation Dose");
pub const MaximumPowerOutputAchieved: Code<'static> = c("122716", "Maximum Power Output Achieved");
pub const MaximumPressureAcceleration: Code<'static> = c("122193", "Maximum pressure acceleration");
pub const MaximumRadiationDose: Code<'static> = c("130004", "Maximum Radiation Dose");
pub const MaximumSlope: Code<'static> = c("126375", "Maximum Slope");
pub const MaximumSurfaceRadiationDose: Code<'static> = c("130002", "Maximum Surface Radiation Dose");
pub const MaximumXRayTubeCurrent: Code<'static> = c("113833", "Maximum X-Ray Tube Current");
pub const MeanAbsorbedRadiationDose: Code<'static> = c("128533", "Mean Absorbed Radiation Dose");
pub const MeanAttenuationCoefficient: Code<'static> = c("112181", "Mean Attenuation Coefficient");
#[deprecated = "retired in PS3.16"]
pub const MeanBloodPressure: Code<'static> = c("109027", "Mean blood pressure");
pub const MeanCTDIfreeair: Code<'static> = c("113837", "Mean CTDIfreeair");
pub const MeanCTDIvol: Code<'static> = c("113830", "Mean CTDIvol");
pub const MeanDiffusivity: Code<'static> = c("113202", "Mean Diffusivity");
pub const MeanEquivalentRadiationDose: Code<'static> = c("128537", "Mean Equivalent Radiation Dose");
pub const MeanIntensityProjection: Code<'static> = c("130924", "Mean intensity projection");
pub const MeanKurtosis: Code<'static> = c("113205", "Mean Kurtosis");
pub const MeanLocalReference: Code<'static> = c("122491", "Mean Local Reference");
pub const MeanTransitTime: Code<'static> = c("113052", "Mean Transit Time");
pub const MeanUltrasoundAttenuationCoefficient: Code<'static> = c("131342", "Mean Ultrasound Attenuation Coefficient");
pub const MeanValueChosen: Code<'static> = c("121412", "Mean value chosen");
pub const MeasuredAPDimension: Code<'static> = c("113932", "Measured AP Dimension");
pub const MeasuredLateralDimension: Code<'static> = c("113931", "Measured Lateral Dimension");
pub const MeasuredProperty: Code<'static> = c("125307", "Measured Property");
pub const MeasuredRadiationDose: Code<'static> = c("128497", "Measured Radiation Dose");
pub const MeasurementDivisor: Code<'static> = c("125308", "Measurement Divisor");
pub const MeasurementFailure: Code<'static> = c("114006", "Measurement failure");
pub const MeasurementFromThisDevice: Code<'static> = c("111780", "Measurement From This Device");
pub const MeasurementGroup: Code<'static> = c("125007", "Measurement Group");
pub const MeasurementNotAttempted: Code<'static> = c("114007", "Measurement not attempted");
pub const MeasurementOfResponse: Code<'static> = c("112051", "Measurement of Response");
pub const MeasurementOrientation: Code<'static> = c("125105", "Measurement Orientation");
pub const MeasurementType: Code<'static> = c("125306", "Measurement Type");
pub const MechanicalBeamSteering: Code<'static> = c("125258", "Mechanical beam steering");
pub const MechanicalDispersion: Code<'static> = c("130673", "Mechanical Dispersion");
pub const MechanicalFailure: Code<'static> = c("111216", "Mechanical failure");
pub const MechanicalPointer: Code<'static> = c("130643", "Mechanical Pointer");
pub const MediaCreationDevice: Code<'static> = c("MCD", "Media Creation Device");
pub const MediaFailure: Code<'static> = c("110524", "Media Failure");
pub const MediaImport: Code<'static> = c("110013", "Media Import");
pub const MedialDissection: Code<'static> = c("122399", "Medial Dissection");
pub const Median: Code<'static> = c("130290", "Median");
pub const MedianAbsorbedRadiationDose: Code<'static> = c("128539", "Median Absorbed Radiation Dose");
pub const MedianAttenuationCoefficient: Code<'static> = c("112182", "Median Attenuation Coefficient");
pub const MedianEquivalentRadiationDose: Code<'static> = c("128540", "Median Equivalent Radiation Dose");
pub const MedianIntensityProjection: Code<'static> = c("130925", "Median intensity projection");
pub const MedianUltrasoundAttenuationCoefficient: Code<'static> = c("131343", "Median Ultrasound Attenuation Coefficient");
pub const MedicalDeviceUsed: Code<'static> = c("121436", "Medical Device Used");
pub const MedicalPhysics: Code<'static> = c("128174", "Medical Physics");
#[deprecated = "retired in PS3.16"]
pub const MedicationHistory: Code<'static> = c("111512", "Medication History");
pub const MedicationType: Code<'static> = c("111516", "Medication Type");
pub const MedicationsSet: Code<'static> = c("127320", "Medications Set");
pub const Medium: Code<'static> = c("111576", "Medium");
pub const Medium320380CmLateralThickness: Code<'static> = c("113602", "Medium: 32.0-38.0 cm lateral thickness");
pub const MenopausalPhase: Code<'static> = c("111539", "Menopausal phase");
pub const MenstrualCyclePhase: Code<'static> = c("111391", "Menstrual Cycle Phase");
pub const MeshRadiationTransportModel: Code<'static> = c("128423", "Mesh Radiation Transport Model");
pub const MesopicLight: Code<'static> = c("111628", "Mesopic light");
pub const MessagingSystem: Code<'static> = c("110159", "Messaging System");
pub const MetabolicVolume: Code<'static> = c("126032", "Metabolic Volume");
pub const MetaboliteMapsFromSpectroscopyData: Code<'static> = c("113050", "Metabolite Maps from spectroscopy data");
pub const MetastasisToAnIntramammaryLymphNode: Code<'static> = c("111333", "Metastasis to an intramammary lymph node");
pub const MetastaticCancerToTheBreast: Code<'static> = c("111323", "Metastatic cancer to the breast");
pub const MetastaticCancerToTheBreastFromTheColon: Code<'static> = c("111324", "Metastatic cancer to the breast from the colon");
pub const MetastaticCancerToTheBreastFromTheLung: Code<'static> = c("111325", "Metastatic cancer to the breast from the lung");
pub const MetastaticCancerToTheBreastFromTheOvary: Code<'static> = c("111327", "Metastatic cancer to the breast from the ovary");
pub const MetastaticDiseaseToAxillaryNode: Code<'static> = c("111330", "Metastatic disease to axillary node");
pub const MetastaticMelanomaToTheBreast: Code<'static> = c("111326", "Metastatic melanoma to the breast");
pub const MetastaticSarcomaToTheBreast: Code<'static> = c("111328", "Metastatic sarcoma to the breast");
pub const Meterset: Code<'static> = c("130797", "Meterset");
pub const MetersetRateInterlock: Code<'static> = c("130830", "Meterset Rate Interlock");
pub const MetersetToleranceViolation: Code<'static> = c("130469", "Meterset Tolerance Violation");
pub const MethodCitation: Code<'static> = c("121423", "Method Citation");
pub const MethodOfDisksBiplane: Code<'static> = c("125207", "Method of Disks, Biplane");
pub const MethodOfDisksSinglePlane: Code<'static> = c("125208", "Method of Disks, Single Plane");
#[deprecated = "retired in PS3.16"]
pub const MicroclipPlaced: Code<'static> = c("111441", "Microclip placed");
pub const MicrodosimetricKineticModel: Code<'static> = c("130129", "Microdosimetric Kinetic Model");
pub const MicroglandularAdenosis: Code<'static> = c("111284", "Microglandular adenosis");
pub const Micronodule: Code<'static> = c("112122", "Micronodule");
pub const MidEsophageal0DegreeTEE: Code<'static> = c("131100", "Mid-esophageal 0 degree TEE");
pub const MidEsophageal135DegreeTEE: Code<'static> = c("131104", "Mid-esophageal 135 degree TEE");
pub const MidEsophageal45DegreeTEE: Code<'static> = c("131101", "Mid-esophageal 45 degree TEE");
pub const MidEsophageal60DegreeTEE: Code<'static> = c("131102", "Mid-esophageal 60 degree TEE");
pub const MidEsophageal90DegreeTEE: Code<'static> = c("131103", "Mid-esophageal 90 degree TEE");
pub const MidPeripheralInferior: Code<'static> = c("111908", "Mid-peripheral-inferior");
pub const MidPeripheralInferiorNasal: Code<'static> = c("111909", "Mid-peripheral-inferior nasal");
pub const MidPeripheralInferiorTemporal: Code<'static> = c("111907", "Mid-peripheral-inferior temporal");
pub const MidPeripheralNasal: Code<'static> = c("111910", "Mid-peripheral-nasal");
pub const MidPeripheralSuperior: Code<'static> = c("111904", "Mid-peripheral-superior");
pub const MidPeripheralSuperiorNasal: Code<'static> = c("111911", "Mid-peripheral-superior nasal");
pub const MidPeripheralSuperiorTemporal: Code<'static> = c("111905", "Mid-peripheral-superior temporal");
pub const MidPeripheralTemporal: Code<'static> = c("111906", "Mid-peripheral-temporal");
pub const MiddleInnerRetinaVasculatureFlow: Code<'static> = c("128267", "Middle inner retina vasculature flow");
pub const MiddleInnerStructuralReflectanceMap: Code<'static> = c("128268", "Middle inner structural reflectance map");
pub const MiddleOfTimePeriod: Code<'static> = c("130535", "Middle of Time Period");
pub const MidlineOfTheEllipsoidZone: Code<'static> = c("128321", "Midline of the ellipsoid zone");
pub const MidlungWindow: Code<'static> = c("112085", "Midlung window");
pub const MidsagittalFacialProfile: Code<'static> = c("131373", "Midsagittal facial profile");
pub const MidsagittalView: Code<'static> = c("131377", "Midsagittal view");
pub const MiliaryPattern: Code<'static> = c("112129", "Miliary pattern");
pub const MinimizeMeterset: Code<'static> = c("130018", "Minimize Meterset");
pub const MinimumAbsoluteVolumeAtRadiationDose: Code<'static> = c("130016", "Minimum Absolute Volume at Radiation Dose");
pub const MinimumAbsorbedRadiationDose: Code<'static> = c("128532", "Minimum Absorbed Radiation Dose");
pub const MinimumAttenuationCoefficient: Code<'static> = c("112179", "Minimum Attenuation Coefficient");
pub const MinimumConformationNumber: Code<'static> = c("130012", "Minimum Conformation Number");
pub const MinimumConformityIndex: Code<'static> = c("130010", "Minimum Conformity Index");
pub const MinimumEquivalentRadiationDose: Code<'static> = c("128536", "Minimum Equivalent Radiation Dose");
pub const MinimumEquivalentUniformDose: Code<'static> = c("130007", "Minimum Equivalent Uniform Dose");
pub const MinimumGanglionCellThickness: Code<'static> = c("131289", "Minimum ganglion cell thickness");
pub const MinimumHealthyTissueConformityIndex: Code<'static> = c("130011", "Minimum Healthy Tissue Conformity Index");
pub const MinimumIntensityProjection: Code<'static> = c("113079", "Minimum intensity projection");
pub const MinimumMeanRadiationDose: Code<'static> = c("130005", "Minimum Mean Radiation Dose");
pub const MinimumPercentVolumeAtRadiationDose: Code<'static> = c("130014", "Minimum Percent Volume at Radiation Dose");
pub const MinimumRadiationDose: Code<'static> = c("130003", "Minimum Radiation Dose");
pub const MinimumSurfaceRadiationDose: Code<'static> = c("130001", "Minimum Surface Radiation Dose");
pub const MitralAnnularExcursion: Code<'static> = c("131180", "Mitral annular excursion");
pub const MitralAnnulusAnterolateralToPosteromedialDiameter: Code<'static> = c("131181", "Mitral annulus anterolateral to posteromedial diameter");
pub const MitralAnnulusAnteroposteriorDiameter: Code<'static> = c("131182", "Mitral annulus anteroposterior diameter");
pub const MitralAnnulusArea: Code<'static> = c("131183", "Mitral annulus area");
pub const MitralAnnulusCalcificationSeverity: Code<'static> = c("131184", "Mitral annulus calcification severity");
pub const MitralAnnulusCommissuralDiameter: Code<'static> = c("131185", "Mitral annulus commissural diameter");
pub const MitralAnnulusDiameterRatio: Code<'static> = c("131186", "Mitral annulus diameter ratio");
pub const MitralAnnulusHeight: Code<'static> = c("131187", "Mitral annulus height");
pub const MitralAnnulusNonplanarityAngle: Code<'static> = c("131188", "Mitral annulus nonplanarity angle");
pub const MitralAnnulusPerimeter: Code<'static> = c("131189", "Mitral annulus perimeter");
pub const MitralAnteriorLeafletA1ScallopLength: Code<'static> = c("131173", "Mitral anterior leaflet A1 scallop length");
pub const MitralAnteriorLeafletA2ScallopLength: Code<'static> = c("131174", "Mitral anterior leaflet A2 scallop length");
pub const MitralAnteriorLeafletA3ScallopLength: Code<'static> = c("131175", "Mitral anterior leaflet A3 scallop length");
pub const MitralAnteriorLeafletArea: Code<'static> = c("131176", "Mitral anterior leaflet area");
pub const MitralCommissureDistance: Code<'static> = c("131178", "Mitral commissure distance");
pub const MitralTrigoneToTrigoneDistance: Code<'static> = c("131179", "Mitral trigone-to-trigone distance");
pub const MitralValveAnnulusDiameter: Code<'static> = c("131060", "Mitral valve annulus diameter");
pub const MitralValveClosure: Code<'static> = c("130699", "Mitral Valve Closure");
pub const MitralValveCoaptationLength: Code<'static> = c("131190", "Mitral valve coaptation length");
pub const MitralValveInterpapillaryDistance: Code<'static> = c("131191", "Mitral valve interpapillary distance");
pub const MitralValveOpening: Code<'static> = c("130700", "Mitral Valve Opening");
pub const MitralValveProlapseArea: Code<'static> = c("131194", "Mitral valve prolapse area");
pub const MitralValveProlapseVolume: Code<'static> = c("131195", "Mitral valve prolapse volume");
pub const MitralValveSegmentFlailGap: Code<'static> = c("131196", "Mitral valve segment flail gap");
pub const MitralValveSphericityIndex: Code<'static> = c("131197", "Mitral valve sphericity index");
pub const MitralValveTentingArea: Code<'static> = c("131199", "Mitral valve tenting area");
pub const MitralValveTentingHeight: Code<'static> = c("131198", "Mitral valve tenting height");
pub const MitralValveTentingSegmentHeightA1P1: Code<'static> = c("131200", "Mitral valve tenting segment height A1-P1");
pub const MitralValveTentingSegmentHeightA2P2: Code<'static> = c("131201", "Mitral valve tenting segment height A2-P2");
pub const MitralValveTentingSegmentHeightA3P3: Code<'static> = c("131202", "Mitral valve tenting segment height A3-P3");
pub const MixedModality3DCAMModel: Code<'static> = c("129019", "Mixed Modality 3D CAM model");
pub const MixedSex: Code<'static> = c("127146", "Mixed sex");
pub const Modality: Code<'static> = c("121139", "Modality");
pub const ModalityToRead: Code<'static> = c("128002", "Modality to Read");
pub const ModeAbsorbedRadiationDose: Code<'static> = c("128534", "Mode Absorbed Radiation Dose");
pub const ModeEquivalentRadiationDose: Code<'static> = c("128538", "Mode Equivalent Radiation Dose");
pub const ModelFittingMethod: Code<'static> = c("113241", "Model fitting method");
pub const ModelFree: Code<'static> = c("113233", "Model Free");
pub const ModelFreeConcentrationTimeQuantitification: Code<'static> = c("126342", "Model-free concentration-time quantitification");
pub const ModelMaximumAge: Code<'static> = c("128430", "Model Maximum Age");
pub const ModelMaximumHeight: Code<'static> = c("128442", "Model Maximum Height");
pub const ModelMaximumWeight: Code<'static> = c("128441", "Model Maximum Weight");
pub const ModelMinimumAge: Code<'static> = c("128428", "Model Minimum Age");
pub const ModelMinimumHeight: Code<'static> = c("128439", "Model Minimum Height");
pub const ModelMinimumWeight: Code<'static> = c("128438", "Model Minimum Weight");
pub const ModelPatientSex: Code<'static> = c("128437", "Model Patient Sex");
pub const ModifiedHounsfieldUnit: Code<'static> = c("129321", "Modified Hounsfield Unit");
pub const ModifiedSimpson: Code<'static> = c("125227", "Modified Simpson");
pub const ModifyingEquipment: Code<'static> = c("109103", "Modifying Equipment");
pub const Mogamulizumab89Zr: Code<'static> = c("126738", "Mogamulizumab ^89^Zr");
pub const Mold: Code<'static> = c("130113", "Mold");
pub const MonoExponentialApparentDiffusionCoefficient: Code<'static> = c("113290", "Mono-exponential Apparent Diffusion Coefficient");
pub const MonoExponentialDiffusionModel: Code<'static> = c("113250", "Mono-exponential diffusion model");
pub const MonoblockStem: Code<'static> = c("112315", "Monoblock Stem");
pub const MonoclonalAntibody64Cu: Code<'static> = c("126510", "Monoclonal Antibody ^64^Cu");
pub const MonoclonalAntibody89Zr: Code<'static> = c("126511", "Monoclonal Antibody ^89^Zr");
pub const MonopoleSignal: Code<'static> = c("109008", "Monopole signal");
pub const MorphologicalOperations: Code<'static> = c("123104", "Morphological Operations");
pub const MosaicPattern: Code<'static> = c("112130", "Mosaic pattern");
pub const MosaicPerfusion: Code<'static> = c("112080", "Mosaic perfusion");
pub const MostRecentValueChosen: Code<'static> = c("121411", "Most recent value chosen");
pub const MotherOfFetus: Code<'static> = c("121036", "Mother of fetus");
pub const MotionBlur: Code<'static> = c("111210", "Motion blur");
pub const MotorizedWedge: Code<'static> = c("130347", "Motorized Wedge");
pub const MouseAlphaSynucleinPreformedFibrils: Code<'static> = c("127852", "Mouse alpha synuclein preformed fibrils");
pub const MouseTauPreformedFibrils: Code<'static> = c("127854", "Mouse Tau preformed fibrils");
pub const MoveableLaserPatientSetupPoint: Code<'static> = c("130071", "Moveable Laser Patient Setup Point");
pub const MultiEnergyMAR: Code<'static> = c("131543", "Multi-Energy MAR");
pub const MultiEnergyProportionalWeighting: Code<'static> = c("113097", "Multi-energy proportional weighting");
pub const MultiMediaCard: Code<'static> = c("110035", "Multi-media Card");
pub const MultiScalePerResolutionFiltering: Code<'static> = c("123106", "Multi-Scale/Resolution Filtering");
pub const MultiSideIllumination: Code<'static> = c("130812", "Multi-side illumination");
pub const MultiTensor: Code<'static> = c("113232", "Multi Tensor");
pub const MultifocalIntraductalCarcinoma: Code<'static> = c("111329", "Multifocal intraductal carcinoma");
pub const MultifocalInvasiveDuctalCarcinoma: Code<'static> = c("111332", "Multifocal invasive ductal carcinoma");
pub const MultiparametricMRI: Code<'static> = c("126020", "Multiparametric MRI");
#[deprecated = "retired in PS3.16"]
pub const MultiparametricMRIOfProstate: Code<'static> = c("126021", "Multiparametric MRI of prostate");
pub const MultiparametricMRIOfWholeBody: Code<'static> = c("126022", "Multiparametric MRI of whole body");
pub const MultiplanarReformatting: Code<'static> = c("113072", "Multiplanar reformatting");
pub const MultipleFillingDefect: Code<'static> = c("111426", "Multiple filling defect");
pub const MultipleFixedSources: Code<'static> = c("130138", "Multiple Fixed Sources");
pub const MultipleIntraductalPapillomas: Code<'static> = c("111285", "Multiple Intraductal Papillomas");
pub const MultipleLADRegionInRAOProjection: Code<'static> = c("122468", "Multiple LAD Region in RAO Projection");
pub const MultiplePlanes: Code<'static> = c("112714", "Multiple planes");
pub const MultipleRCARegionInRAOProjection: Code<'static> = c("122469", "Multiple RCA Region in RAO Projection");
pub const MultipleShotEPI: Code<'static> = c("113227", "Multiple Shot EPI");
pub const MultipleSlices: Code<'static> = c("122562", "Multiple Slices");
pub const MultiplexGroupNumber: Code<'static> = c("130880", "Multiplex Group Number");
pub const MultiplexGroupUID: Code<'static> = c("130881", "Multiplex Group UID");
pub const MultispectralProcessing: Code<'static> = c("123108", "Multispectral Processing");
pub const MusculoskeletalImagingSpecialty: Code<'static> = c("128009", "Musculoskeletal Imaging Specialty");
pub const MusculoskeletalImagingSubjectMatter: Code<'static> = c("128728", "Musculoskeletal imaging subject matter");
pub const MusculoskeletalRadiology: Code<'static> = c("128175", "Musculoskeletal Radiology");
pub const MyocardialBiopsy: Code<'static> = c("122057", "Myocardial biopsy");
pub const MyocardialMidlayerMethod: Code<'static> = c("130685", "Myocardial Midlayer Method");
pub const MyocardialPerfusionAnalysis: Code<'static> = c("122602", "Myocardial Perfusion Analysis");
#[deprecated = "retired in PS3.16"]
pub const Myofibroblastoma: Code<'static> = c("111283", "Myofibroblastoma");
pub const N0DocumentedAbsenceOfCAD: Code<'static> = c("130723", "0 - Documented absence of CAD");
pub const N100MmPediatricHeadCTDosimetryPhantom: Code<'static> = c("130541", "100 mm Pediatric Head CT Dosimetry Phantom");
pub const N100cmFromXRaySource: Code<'static> = c("113965", "100cm from X-Ray Source");
pub const N10X: Code<'static> = c("112716", "10X");
pub const N10YearCHDRisk: Code<'static> = c("122230", "10 Year CHD Risk");
pub const N12CarbonNucleus: Code<'static> = c("130143", "^12^Carbon nucleus");
pub const N152Terbium: Code<'static> = c("126606", "^152^Terbium");
#[deprecated = "retired in PS3.16"]
pub const N15LeadECG: Code<'static> = c("122062", "15-Lead ECG");
pub const N15cmFromIsocenterTowardSource: Code<'static> = c("113860", "15cm from Isocenter toward Source");
pub const N15cmFromTableCenterline: Code<'static> = c("113864", "15cm from Table Centerline");
pub const N16OxygenNucleus: Code<'static> = c("130144", "^16^Oxygen nucleus");
pub const N191mIridium: Code<'static> = c("126604", "^191m^Iridium");
pub const N1MinimalNonObstructiveCAD: Code<'static> = c("130724", "1 - Minimal non-obstructive CAD");
pub const N1cmAboveTabletop: Code<'static> = c("113862", "1cm above Tabletop");
pub const N1stWeek: Code<'static> = c("111392", "1st week");
pub const N20X: Code<'static> = c("112717", "20X");
pub const N28H189Zr: Code<'static> = c("126752", "28H1 ^89^Zr");
pub const N2DShearWaveElastography: Code<'static> = c("130609", "2D Shear Wave Elastography");
pub const N2DSpeckleTracking: Code<'static> = c("130679", "2D Speckle Tracking");
pub const N2FAF18: Code<'static> = c("126713", "2FA F^18^");
pub const N2MildNonObstructiveCAD: Code<'static> = c("130725", "2 - Mild non-obstructive CAD");
pub const N2SigmaDeviationOfPopulation: Code<'static> = c("121417", "2 Sigma deviation of population");
pub const N2ndWeek: Code<'static> = c("111393", "2nd week");
pub const N30cmAboveTabletop: Code<'static> = c("113863", "30cm above Tabletop");
pub const N30cmInFrontOfImageInputSurface: Code<'static> = c("113861", "30cm in Front of Image Input Surface");
pub const N35PercentOfThermalPerDyeDilutionCO: Code<'static> = c("109014", "35% of thermal/dye dilution CO");
pub const N3DDoseMap: Code<'static> = c("128487", "3D Dose Map");
pub const N3DGel: Code<'static> = c("128701", "3D Gel");
pub const N3DManufacturingModelingSystem: Code<'static> = c("M3D", "3D Manufacturing Modeling System");
#[deprecated = "retired in PS3.16"]
pub const N3DMode: Code<'static> = c("125231", "3D mode");
pub const N3DShearWaveElastography: Code<'static> = c("130610", "3D Shear Wave Elastography");
pub const N3DSpeckleTracking: Code<'static> = c("130680", "3D Speckle Tracking");
pub const N3HeliumNucleus: Code<'static> = c("130141", "^3^Helium nucleus");
pub const N3ModerateStenosis: Code<'static> = c("130726", "3 - Moderate stenosis");
pub const N3rdWeek: Code<'static> = c("111394", "3rd week");
pub const N40X: Code<'static> = c("112718", "40X");
pub const N42cmAboveBreastSupportSurface: Code<'static> = c("113865", "4.2cm above Breast Support Surface");
pub const N43Scandium: Code<'static> = c("126605", "^43^Scandium");
pub const N44Scandium: Code<'static> = c("126600", "^44^Scandium");
pub const N4ASevereStenosis: Code<'static> = c("130728", "4A - Severe stenosis");
pub const N4BSevereStenosis: Code<'static> = c("130729", "4B - Severe stenosis");
pub const N4DRadiationTreatmentNote: Code<'static> = c("130027", "4D Radiation Treatment Note");
pub const N4HeliumNucleus: Code<'static> = c("130142", "^4^Helium nucleus");
pub const N4MonthsTo1YearAgo: Code<'static> = c("111397", "4 months to 1 year ago");
pub const N4PointSegmentFindingScale: Code<'static> = c("125223", "4 Point Segment Finding Scale");
pub const N4SevereStenosis: Code<'static> = c("130727", "4 - Severe stenosis");
pub const N51Manganese: Code<'static> = c("126601", "^51^Manganese");
pub const N52mManganese: Code<'static> = c("126607", "^52m^Manganese");
pub const N5PointSegmentFindingScale: Code<'static> = c("125224", "5 Point Segment Finding Scale");
pub const N5PointSegmentFindingScaleWithGradedHypokinesis: Code<'static> = c("125225", "5 Point Segment Finding Scale With Graded Hypokinesis");
pub const N5TotalCoronaryOcclusion: Code<'static> = c("130730", "5 - Total coronary occlusion");
pub const N5X: Code<'static> = c("112715", "5X");
pub const N70Arsenic: Code<'static> = c("126602", "^70^Arsenic");
pub const N70PercentOfThermalPerDyeDilutionCO: Code<'static> = c("109015", "70% of thermal/dye dilution CO");
pub const N7D1289Zr: Code<'static> = c("126751", "7D12 ^89^Zr");
pub const N7E1189Zr: Code<'static> = c("126750", "7E11 ^89^Zr");
pub const N90Niobium: Code<'static> = c("126603", "^90^Niobium");
pub const NASCET: Code<'static> = c("122655", "NASCET");
pub const NAcetylaspartatePerCholineRatio: Code<'static> = c("113083", "N-acetylaspartate/Choline Ratio");
pub const NAcetylaspartatePerCreatineRatio: Code<'static> = c("113082", "N-acetylaspartate/Creatine Ratio");
pub const NEMAXR212000Phantom: Code<'static> = c("113692", "NEMA XR21-2000 Phantom");
pub const NIH07: Code<'static> = c("127271", "NIH07");
pub const NIH31: Code<'static> = c("127270", "NIH31");
pub const NMMyocardialViabilityProcedure: Code<'static> = c("122785", "NM Myocardial Viability procedure");
pub const NMTransmissionAttenuationCorrection: Code<'static> = c("122727", "NM transmission attenuation correction");
pub const NNonDiagnosticSegments: Code<'static> = c("130732", "N - Non-diagnostic segments");
pub const NNonDiagnosticStudy: Code<'static> = c("130731", "N - Non-diagnostic study");
pub const NURBSRadiationTransportModel: Code<'static> = c("128424", "NURBS Radiation Transport Model");
pub const NanocolloidalAlbumin89Zr: Code<'static> = c("126753", "Nanocolloidal albumin ^89^Zr");
pub const NarrativeSummary: Code<'static> = c("111412", "Narrative Summary");
pub const NasalCannula: Code<'static> = c("127061", "Nasal cannula");
pub const NationalKennelClub: Code<'static> = c("109215", "National Kennel Club");
pub const NativePlaqueVolume: Code<'static> = c("122375", "Native Plaque Volume");
pub const NearPupillaryDistance: Code<'static> = c("111680", "Near Pupillary Distance");
pub const Nearline: Code<'static> = c("NEARLINE", "Nearline");
pub const NeckComponent: Code<'static> = c("112314", "Neck Component");
pub const NecroticLipidicPlaque: Code<'static> = c("122395", "Necrotic-Lipidic Plaque");
pub const NeedleGauge: Code<'static> = c("111465", "Needle Gauge");
pub const NeedleInTarget: Code<'static> = c("111438", "Needle in target");
pub const NeedleLength: Code<'static> = c("111467", "Needle Length");
pub const NeedleLocalizationAndBiopsy: Code<'static> = c("111144", "Needle localization and biopsy");
pub const NegativeDCE: Code<'static> = c("130601", "Negative DCE");
pub const NegativeEnhancementIntegral: Code<'static> = c("113054", "Negative Enhancement Integral");
pub const NegativeExponential: Code<'static> = c("130252", "Negative exponential");
pub const NegativeInfinity: Code<'static> = c("114001", "Negative Infinity");
pub const NeighborhoodAnalysis: Code<'static> = c("123101", "Neighborhood Analysis");
pub const NeighbourhoodGreyToneDifferenceMatrix: Code<'static> = c("128779", "Neighbourhood Grey Tone Difference Matrix");
pub const NeighbouringGreyLevelDependenceMatrix: Code<'static> = c("128780", "Neighbouring Grey Level Dependence Matrix");
#[deprecated = "retired in PS3.16"]
pub const NeoplasmOfTheMammarySkin: Code<'static> = c("111335", "Neoplasm of the mammary skin");
pub const NetForwardVolume: Code<'static> = c("122645", "Net Forward Volume");
pub const NetworkConfiguration: Code<'static> = c("110128", "Network Configuration");
pub const NetworkEntry: Code<'static> = c("110108", "Network Entry");
#[deprecated = "retired in PS3.16"]
pub const Neurofibromatosis: Code<'static> = c("111288", "Neurofibromatosis");
pub const NeuroimagingSubjectMatter: Code<'static> = c("128733", "Neuroimaging subject matter");
pub const NeurologySpecialty: Code<'static> = c("128010", "Neurology Specialty");
pub const NeurophysiologyAutomatedAnalysisAnnotations: Code<'static> = c("130869", "Neurophysiology Automated Analysis Annotations");
pub const NeurophysiologyPostHocReviewAnnotations: Code<'static> = c("130868", "Neurophysiology Post-hoc Review Annotations");
pub const NeurophysiologyRecordingAnnotations: Code<'static> = c("130867", "Neurophysiology Recording Annotations");
pub const NeuroradiologyImagingSpecialty: Code<'static> = c("128011", "Neuroradiology Imaging Specialty");
pub const NeuroretinalRimArea: Code<'static> = c("131259", "Neuroretinal rim area");
pub const NeuroretinalRimWidth: Code<'static> = c("131260", "Neuroretinal rim width");
pub const NeutralMusculoskeletalPosition: Code<'static> = c("109136", "Neutral musculoskeletal position");
pub const NifeneF18: Code<'static> = c("126714", "Nifene F^18^");
pub const NilPerOsNPOStatusConfirmed: Code<'static> = c("122006", "Nil Per Os (NPO) status confirmed");
#[deprecated = "retired in PS3.16"]
pub const NiobiumOrNiobiumCompound: Code<'static> = c("113710", "Niobium or Niobium compound");
pub const NippleCharacteristic: Code<'static> = c("111297", "Nipple Characteristic");
pub const NippleDischargeCytology: Code<'static> = c("111564", "Nipple discharge cytology");
pub const NippleInvolved: Code<'static> = c("111472", "Nipple involved");
pub const NippleNotInProfile: Code<'static> = c("111205", "Nipple not in profile");
pub const NippleRing: Code<'static> = c("112177", "Nipple ring");
pub const NoAbnormality: Code<'static> = c("111286", "No abnormality");
pub const NoAlgorithmsSucceededWithoutFindings: Code<'static> = c("111245", "No algorithms succeeded; without findings");
pub const NoAttenuationCorrection: Code<'static> = c("122729", "No Attenuation Correction");
pub const NoComplications: Code<'static> = c("111492", "No complications");
pub const NoCornealCompensation: Code<'static> = c("111922", "No corneal compensation");
pub const NoCorrelationToClinicalFindings: Code<'static> = c("111387", "No correlation to clinical findings");
pub const NoCorrelationToOtherImagingFindings: Code<'static> = c("111386", "No correlation to other imaging findings");
pub const NoEmptyTileSuppression: Code<'static> = c("112721", "No empty tile suppression");
pub const NoFamilyHistoryOfProstateCancer: Code<'static> = c("130587", "No family history of prostate cancer");
pub const NoFilter: Code<'static> = c("111609", "No filter");
pub const NoGrid: Code<'static> = c("111646", "No grid");
pub const NoImage: Code<'static> = c("111213", "No image");
pub const NoKnownExposure: Code<'static> = c("111587", "No known exposure");
pub const NoModulation: Code<'static> = c("114100", "No modulation");
pub const NoPosteriorAcousticFeatures: Code<'static> = c("111367", "No posterior acoustic features");
pub const NoRealignment: Code<'static> = c("122477", "No Realignment");
pub const NoSensitiveContentIdentified: Code<'static> = c("131390", "No Sensitive Content Identified");
pub const NoSubsequentWorkitems: Code<'static> = c("110009", "No subsequent Workitems");
pub const NodeAuthentication: Code<'static> = c("110126", "Node Authentication");
pub const NodeID: Code<'static> = c("110182", "Node ID");
pub const NodularPattern: Code<'static> = c("112067", "Nodular pattern");
pub const NoiseIndex: Code<'static> = c("131551", "Noise Index");
pub const Nomenclature: Code<'static> = c("127413", "Nomenclature");
pub const NominalEmptyTileSuppression: Code<'static> = c("112719", "Nominal empty tile suppression");
pub const NominalImagingSourceLocation: Code<'static> = c("130789", "Nominal Imaging Source Location");
pub const NominalRadiationSourceLocation: Code<'static> = c("130358", "Nominal Radiation Source Location");
pub const NominalSingleCollimationWidth: Code<'static> = c("113826", "Nominal Single Collimation Width");
pub const NominalTotalCollimationWidth: Code<'static> = c("113827", "Nominal Total Collimation Width");
pub const NonBloodyDischargeFromNipple: Code<'static> = c("111478", "Non-bloody discharge (from nipple)");
pub const NonDiagnosticECG: Code<'static> = c("122753", "Non-diagnostic ECG");
pub const NonDiagnosticLowHeartRate: Code<'static> = c("122750", "Non-diagnostic - low heart rate");
pub const NonDiagnosticRestingSTAbnormalities: Code<'static> = c("122751", "Non-diagnostic - resting ST abnormalities");
pub const NonDiagnosticVentricularPacingOrLBBB: Code<'static> = c("122752", "Non-diagnostic - ventricular pacing or LBBB");
pub const NonFlatteningFilterBeam: Code<'static> = c("130356", "Non-Flattening Filter Beam");
pub const NonFocalAbnormality: Code<'static> = c("130592", "Non-focal abnormality");
pub const NonImagingDopplerUltrasoundTransducerGeometry: Code<'static> = c("125251", "Non-imaging Doppler ultrasound transducer geometry");
pub const NonIonicIodinatedContrastAgent: Code<'static> = c("127855", "Non-ionic iodinated contrast agent");
pub const NonLesion: Code<'static> = c("111102", "Non-lesion");
pub const NonLesionAtBaseline: Code<'static> = c("112076", "Non-Lesion at Baseline");
pub const NonLesionModifier: Code<'static> = c("112037", "Non-lesion Modifier");
pub const NonSpecificVolume: Code<'static> = c("130046", "Non-specific Volume");
pub const NonSynchronizedRoboticTreatment: Code<'static> = c("130140", "Non-Synchronized Robotic Treatment");
pub const NonTargetLesionAtBaseline: Code<'static> = c("112075", "Non-Target Lesion at Baseline");
pub const NonTargetLesionCompleteResponse: Code<'static> = c("112045", "Non-Target Lesion Complete Response");
pub const NonTargetLesionIncompleteResponseOrStableDisease: Code<'static> = c("112046", "Non-Target Lesion Incomplete Response or Stable Disease");
pub const NonTargetLesionProgressiveDisease: Code<'static> = c("112047", "Non-Target Lesion Progressive Disease");
pub const NonUterineLeiomyosarcoma: Code<'static> = c("130406", "Non-uterine leiomyosarcoma");
pub const NormalAxillaryNode: Code<'static> = c("111251", "Normal axillary node");
pub const NormalBreastTissue: Code<'static> = c("111287", "Normal breast tissue");
pub const NormalImplants: Code<'static> = c("111503", "Normal implants");
pub const NormalIntervalFollowUp: Code<'static> = c("111140", "Normal interval follow-up");
pub const NormalMyocardium: Code<'static> = c("122112", "Normal Myocardium");
pub const NormalRangeAuthority: Code<'static> = c("121408", "Normal Range Authority");
pub const NormalRangeDescription: Code<'static> = c("121407", "Normal Range description");
pub const Normality: Code<'static> = c("121402", "Normality");
pub const NormalizationFactor: Code<'static> = c("128522", "Normalization Factor");
pub const NormalizedChordLength: Code<'static> = c("122450", "Normalized Chord Length");
pub const NormalizedValuesOfVentricularMeasurements: Code<'static> = c("122609", "Normalized Values Of Ventricular Measurements");
pub const NorthAmericanPurebredDogRegistry: Code<'static> = c("109216", "North American Purebred Dog Registry");
pub const NoseCone: Code<'static> = c("127060", "Nose cone");
pub const NotANumber: Code<'static> = c("114000", "Not a number");
pub const NotAllAlgorithmsSucceededWithFindings: Code<'static> = c("111244", "Not all algorithms succeeded; with findings");
pub const NotAllAlgorithmsSucceededWithoutFindings: Code<'static> = c("111243", "Not all algorithms succeeded; without findings");
pub const NotAttempted: Code<'static> = c("111225", "Not Attempted");
pub const NotForPresentationRenderingDeviceExpectedNotToPresent: Code<'static> = c("111152", "Not for Presentation: Rendering device expected not to present");
pub const NotOptimizedForTheDeviceInstance: Code<'static> = c("128620", "Not optimized for the device instance");
pub const NotParallel: Code<'static> = c("111356", "Not parallel");
pub const NotVisualized: Code<'static> = c("122288", "Not Visualized");
pub const NuclearMedicine: Code<'static> = c("NM", "Nuclear Medicine");
pub const NuclearMedicineImagingSubjectMatter: Code<'static> = c("128735", "Nuclear medicine imaging subject matter");
pub const NuclearMedicineProjectionActivity: Code<'static> = c("110820", "Nuclear Medicine Projection Activity");
pub const NuclearMedicineTomographicActivity: Code<'static> = c("110821", "Nuclear Medicine Tomographic Activity");
pub const NudityContent: Code<'static> = c("131391", "Nudity Content");
pub const NumberOfAnimalsWithinSameHousingUnit: Code<'static> = c("127143", "Number of animals within same housing unit");
pub const NumberOfCalcifications: Code<'static> = c("111038", "Number of calcifications");
pub const NumberOfChannels: Code<'static> = c("130883", "Number of Channels");
pub const NumberOfDiseasedVesselTerritories: Code<'static> = c("122762", "Number of diseased vessel territories");
pub const NumberOfEctopicBeats: Code<'static> = c("122707", "Number of Ectopic Beats");
#[deprecated = "retired in PS3.16"]
pub const NumberOfFetuses: Code<'static> = c("121038", "Number of Fetuses");
pub const NumberOfFirstDegreeRelativesAffectedByMalignantMelanoma: Code<'static> = c("130487", "Number of first-degree relatives affected by malignant melanoma");
pub const NumberOfFocalPlanes: Code<'static> = c("112707", "Number of focal planes");
pub const NumberOfFolliclesInRange: Code<'static> = c("130906", "Number of Follicles in Range");
pub const NumberOfFractionsCompleted: Code<'static> = c("121387", "Number of Fractions Completed");
pub const NumberOfFractionsPlanned: Code<'static> = c("121386", "Number of Fractions Planned");
pub const NumberOfFrames: Code<'static> = c("121140", "Number of Frames");
pub const NumberOfHistories: Code<'static> = c("131325", "Number of Histories");
pub const NumberOfHousingUnitsPerRack: Code<'static> = c("127141", "Number of housing units per rack");
pub const NumberOfImagesUsedForMacularMeasurements: Code<'static> = c("111691", "Number of Images Used for Macular Measurements");
pub const NumberOfInjectorHeads: Code<'static> = c("130219", "Number of Injector Heads");
pub const NumberOfLesionInterventionsAttempted: Code<'static> = c("122175", "Number of lesion interventions attempted");
pub const NumberOfLesionInterventionsSuccessful: Code<'static> = c("122176", "Number of lesion interventions successful");
pub const NumberOfMalignantMelanomas: Code<'static> = c("130483", "Number of malignant melanomas");
pub const NumberOfMelanomasInSitu: Code<'static> = c("130484", "Number of melanomas in situ");
pub const NumberOfNeedlesAroundTarget: Code<'static> = c("111439", "Number of needles around target");
pub const NumberOfNodesPositive: Code<'static> = c("111474", "Number of nodes positive");
pub const NumberOfNodesRemoved: Code<'static> = c("111473", "Number of nodes removed");
pub const NumberOfPasses: Code<'static> = c("111436", "Number of passes");
pub const NumberOfPixels: Code<'static> = c("130922", "Number of pixels");
pub const NumberOfPulses: Code<'static> = c("113768", "Number of Pulses");
pub const NumberOfRacksPerRoom: Code<'static> = c("127140", "Number of racks per room");
pub const NumberOfSamplesUsedPerImage: Code<'static> = c("111692", "Number of Samples Used per Image");
pub const NumberOfSeriesRelatedInstances: Code<'static> = c("131564", "Number of Series Related Instances");
pub const NumberOfSimilarFindings: Code<'static> = c("111406", "Number of similar findings");
pub const NumberOfSpecimens: Code<'static> = c("111437", "Number of specimens");
pub const NumberOfStimulusEvents: Code<'static> = c("130494", "Number of Stimulus Events");
pub const NumberOfStudyRelatedSeries: Code<'static> = c("131565", "Number of Study Related Series");
pub const NumberOfVoxels: Code<'static> = c("130923", "Number of voxels");
pub const NumberOfXRaySources: Code<'static> = c("113823", "Number of X-Ray Sources");
pub const NumericalAcousticModel: Code<'static> = c("130822", "Numerical Acoustic Model");
#[deprecated = "retired in PS3.16"]
pub const Nurse: Code<'static> = c("121082", "Nurse");
pub const NursingNote: Code<'static> = c("121172", "Nursing Note");
pub const NursingUnitCancel: Code<'static> = c("110511", "Nursing unit cancel");
pub const O10: Code<'static> = c("130717", "O10");
pub const O9: Code<'static> = c("130716", "O9");
pub const OBGYNUltrasoundProcedureReport: Code<'static> = c("125000", "OB-GYN Ultrasound Procedure Report");
pub const OBPerGynImagingSpecialty: Code<'static> = c("128012", "OB/Gyn Imaging Specialty");
pub const OCTAAmplitudeDecorrelation: Code<'static> = c("128252", "OCT-A amplitude decorrelation");
pub const OCTAComplexVariance: Code<'static> = c("128253", "OCT-A complex variance");
pub const OCTACorrelationMapping: Code<'static> = c("128255", "OCT-A correlation mapping");
pub const OCTAOneSidedRatioGreater: Code<'static> = c("128305", "OCT-A one-sided ratio (greater)");
pub const OCTAOneSidedRatioLesser: Code<'static> = c("128304", "OCT-A one-sided ratio (lesser)");
pub const OCTASpeckleVariance: Code<'static> = c("128254", "OCT-A speckle variance");
pub const OCTBScanAnalysis: Code<'static> = c("128303", "OCT B-scan analysis");
pub const OIQPattern: Code<'static> = c("109901", "OIQ Pattern");
pub const OLED: Code<'static> = c("109994", "OLED");
pub const OLINDAEXM: Code<'static> = c("113527", "OLINDA-EXM");
pub const ORCCStructuralReflectanceMap: Code<'static> = c("128317", "ORCC structural reflectance map");
pub const ORCCVasculatureFlow: Code<'static> = c("128316", "ORCC vasculature flow");
pub const OSEMAlgorithm: Code<'static> = c("122720", "OSEM algorithm");
pub const OSLD: Code<'static> = c("128706", "OSLD");
pub const Obinituzimab89Zr: Code<'static> = c("126721", "Obinituzimab ^89^Zr");
pub const ObjectRemoved: Code<'static> = c("122307", "Object removed");
pub const ObjectSecurityAttributesChanged: Code<'static> = c("110135", "Object Security Attributes Changed");
pub const ObjectSetIncomplete: Code<'static> = c("110523", "Object Set incomplete");
pub const ObjectType: Code<'static> = c("111039", "Object type");
pub const ObjectTypesNotSupported: Code<'static> = c("110522", "Object Types not supported");
pub const ObjectsIncorrectlyFormatted: Code<'static> = c("110521", "Objects incorrectly formatted");
pub const ObliqueIllumination: Code<'static> = c("111746", "Oblique illumination");
pub const ObliqueShortAxisViewAtDuctusArteriosus: Code<'static> = c("131023", "Oblique short axis view at ductus arteriosus");
pub const ObservationByExaminer: Code<'static> = c("111846", "Observation by Examiner");
pub const ObservationDateTimeQualifier: Code<'static> = c("121135", "Observation DateTime Qualifier");
pub const ObserverType: Code<'static> = c("121005", "Observer Type");
pub const Ocaratuzumab89Zr: Code<'static> = c("126723", "Ocaratuzumab ^89^Zr");
pub const OccultBloodTestResult: Code<'static> = c("111455", "Occult blood test result");
pub const OccultCarcinomaPresentingWithAxillaryLymphNodeMetastases: Code<'static> = c("111322", "Occult carcinoma presenting with axillary lymph node metastases");
pub const OcularFixationLight: Code<'static> = c("130649", "Ocular Fixation Light");
pub const OcularGazeSetupMethod: Code<'static> = c("130635", "Ocular Gaze Setup Method");
pub const OfInterest: Code<'static> = c("113000", "Of Interest");
pub const OfficeCalled: Code<'static> = c("122045", "Office called");
pub const Offline: Code<'static> = c("OFFLINE", "Offline");
pub const OffsetFactor: Code<'static> = c("128523", "Offset Factor");
pub const OilCystFatNecrosisCyst: Code<'static> = c("111290", "Oil cyst (fat necrosis cyst)");
pub const OldFilmsForComparison: Code<'static> = c("111138", "Old films for comparison");
pub const Oligemia: Code<'static> = c("112060", "Oligemia");
pub const Olsen: Code<'static> = c("111765", "Olsen");
pub const OnAdmissionToUnit: Code<'static> = c("109120", "On admission to unit");
pub const OnDischarge: Code<'static> = c("109121", "On discharge");
pub const OnDischargeFromUnit: Code<'static> = c("109122", "On discharge from unit");
pub const OncologicImagingSpecialty: Code<'static> = c("128013", "Oncologic Imaging Specialty");
pub const OncologyMeasurementReport: Code<'static> = c("126001", "Oncology Measurement Report");
pub const OncologySpecialty: Code<'static> = c("128014", "Oncology Specialty");
pub const Ongoing: Code<'static> = c("111528", "Ongoing");
pub const Online: Code<'static> = c("ONLINE", "Online");
pub const OnsetOfExercise: Code<'static> = c("125236", "Onset of exercise");
pub const OnsetOfStimulation: Code<'static> = c("125238", "Onset of stimulation");
pub const Opacity: Code<'static> = c("112001", "Opacity");
pub const OpacityDescriptor: Code<'static> = c("112027", "Opacity Descriptor");
pub const OperatorDecisionToTerminateTreatment: Code<'static> = c("130450", "Operator decision to terminate treatment");
pub const OperatorError: Code<'static> = c("110519", "Operator Error");
pub const OperatorsNarrative: Code<'static> = c("109111", "Operator's narrative");
pub const OphthalmicAxialMeasurements: Code<'static> = c("OAM", "Ophthalmic Axial Measurements");
pub const OphthalmicImageROIMeasurements: Code<'static> = c("131246", "Ophthalmic Image ROI Measurements");
pub const OphthalmicMacularGridProblem: Code<'static> = c("111698", "Ophthalmic Macular Grid Problem");
pub const OphthalmicMapping: Code<'static> = c("OPM", "Ophthalmic Mapping");
pub const OphthalmicPhotography: Code<'static> = c("OP", "Ophthalmic photography");
pub const OphthalmicRefraction: Code<'static> = c("OPR", "Ophthalmic Refraction");
pub const OphthalmicTomography: Code<'static> = c("OPT", "Ophthalmic Tomography");
pub const OphthalmicTomographyBScanVolumeAnalysis: Code<'static> = c("OPTBSV", "Ophthalmic Tomography B-scan Volume Analysis");
pub const OphthalmicTomographyEnFace: Code<'static> = c("OPTENF", "Ophthalmic Tomography En Face");
pub const OphthalmicVisualField: Code<'static> = c("OPV", "Ophthalmic Visual Field");
pub const OppositeRegionSeverity: Code<'static> = c("122461", "Opposite Region Severity");
pub const OpticCupArea: Code<'static> = c("131261", "Optic cup area");
pub const OpticCupVolume: Code<'static> = c("131263", "Optic cup volume");
pub const OpticDiscArea: Code<'static> = c("131262", "Optic disc area");
pub const OpticDiscKeyMeasurements: Code<'static> = c("131241", "Optic Disc Key Measurements");
pub const Optical: Code<'static> = c("111752", "Optical");
pub const OpticalCoherenceTomography: Code<'static> = c("OCT", "Optical Coherence Tomography");
pub const OpticalDistanceMeter: Code<'static> = c("130642", "Optical Distance Meter");
pub const OpticalFilterType: Code<'static> = c("112712", "Optical filter type");
pub const OpticalFixationMeasurements: Code<'static> = c("111856", "Optical Fixation Measurements");
pub const OpticalSurfaceScanner: Code<'static> = c("OSS", "Optical Surface Scanner");
pub const OptimizedForTheDeviceInstance: Code<'static> = c("128608", "Optimized for the device instance");
pub const OrderRecord: Code<'static> = c("110109", "Order Record");
pub const OrganAtRisk: Code<'static> = c("130060", "Organ At Risk");
pub const OrganBasedModulation: Code<'static> = c("114104", "Organ-based modulation");
pub const OrganDose: Code<'static> = c("113518", "Organ Dose");
pub const OrganDoseInformation: Code<'static> = c("113517", "Organ Dose Information");
pub const OrganizationName: Code<'static> = c("113873", "Organization Name");
pub const Orientation: Code<'static> = c("111354", "Orientation");
pub const OrientationDescriptor: Code<'static> = c("112014", "Orientation Descriptor");
pub const OriginalSource: Code<'static> = c("111040", "Original Source");
pub const OrthognathicFunctionalCondition: Code<'static> = c("130325", "Orthognathic Functional Condition");
pub const OrthogonalLocationArc: Code<'static> = c("113669", "Orthogonal location arc");
pub const OrthogonalLocationArcInnerMargin: Code<'static> = c("113670", "Orthogonal location arc inner margin");
pub const OrthogonalLocationArcOuterMargin: Code<'static> = c("113671", "Orthogonal location arc outer margin");
pub const OsmolalityAt37C: Code<'static> = c("130184", "Osmolality at 37C");
pub const OsmolarityAt37C: Code<'static> = c("130185", "Osmolarity at 37C");
pub const Osseous: Code<'static> = c("112053", "Osseous");
pub const OsseousModifier: Code<'static> = c("112038", "Osseous Modifier");
pub const OstiumOfLeftAuricularAppendage: Code<'static> = c("131120", "Ostium of Left Auricular Appendage");
pub const OtherFailure: Code<'static> = c("111220", "Other failure");
pub const OtherImageOfBiplanePair: Code<'static> = c("121314", "Other image of biplane pair");
pub const OtherImageOfStereoscopicPair: Code<'static> = c("121315", "Other image of stereoscopic pair");
pub const OtherMalignancyType: Code<'static> = c("111390", "Other malignancy type");
pub const OtherMarker: Code<'static> = c("111175", "Other Marker");
pub const OtherModality: Code<'static> = c("OT", "Other Modality");
pub const OtherPartialViews: Code<'static> = c("121313", "Other partial views");
pub const OtherProjectionSystem: Code<'static> = c("109999", "Other Projection System");
pub const OtherSex: Code<'static> = c("121102", "Other sex");
pub const OutOfFocus: Code<'static> = c("131504", "Out of focus");
pub const OutOfToleranceInterlock: Code<'static> = c("130743", "Out of Tolerance Interlock");
pub const OuterLimitsOfFuzzyMargin: Code<'static> = c("113663", "Outer limits of fuzzy margin");
pub const OuterRetinaStructuralReflectanceMap: Code<'static> = c("128272", "Outer retina structural reflectance map");
pub const OuterRetinaVasculatureFlow: Code<'static> = c("128271", "Outer retina vasculature flow");
pub const OuterSurfaceOfBruchsMembrane: Code<'static> = c("128300", "Outer surface of Bruch’s Membrane");
pub const OuterSurfaceOfGCL: Code<'static> = c("128290", "Outer surface of GCL");
pub const OuterSurfaceOfHFL: Code<'static> = c("128294", "Outer surface of HFL");
pub const OuterSurfaceOfINL: Code<'static> = c("128292", "Outer surface of INL");
pub const OuterSurfaceOfIPL: Code<'static> = c("128291", "Outer surface of IPL");
pub const OuterSurfaceOfOPL: Code<'static> = c("128293", "Outer surface of OPL");
pub const OuterSurfaceOfRNFL: Code<'static> = c("128289", "Outer surface of RNFL");
pub const OuterSurfaceOfTheCC: Code<'static> = c("128302", "Outer surface of the CC");
pub const OuterSurfaceOfTheEllipsoidZone: Code<'static> = c("128322", "Outer surface of the ellipsoid zone");
pub const OuterSurfaceOfTheInterdigitationZone: Code<'static> = c("128324", "Outer surface of the interdigitation zone");
pub const OuterSurfaceOfTheRPE: Code<'static> = c("128299", "Outer surface of the RPE");
pub const OutflowAngle: Code<'static> = c("122547", "Outflow Angle");
pub const Outline: Code<'static> = c("111041", "Outline");
pub const OutlineOfLobulations: Code<'static> = c("113661", "Outline of lobulations");
pub const OutlineOfSpiculations: Code<'static> = c("113664", "Outline of spiculations");
pub const OutputMeasurementPointPosition: Code<'static> = c("130525", "Output Measurement Point Position");
pub const OutsideNormalLimits: Code<'static> = c("111847", "Outside normal limits");
pub const OutsideOperationalLimitsOfTheDevice: Code<'static> = c("128619", "Outside operational limits of the device");
pub const OverExposed: Code<'static> = c("111212", "Over exposed");
pub const OverallAssessment: Code<'static> = c("111413", "Overall Assessment");
pub const OverallImageQualityEvaluation: Code<'static> = c("109701", "Overall image quality evaluation");
pub const OverallImpressionPerRecommendationAnalysis: Code<'static> = c("111234", "Overall Impression/\u{200b}Recommendation Analysis");
pub const OverallProstateFinding: Code<'static> = c("130554", "Overall Prostate Finding");
pub const OverallStudyQuality: Code<'static> = c("122739", "Overall study quality");
pub const Overflow: Code<'static> = c("114005", "Overflow");
pub const OxygenAdministrationByVentilator: Code<'static> = c("121163", "Oxygen Administration by ventilator");
pub const OxygenAdministrationRate: Code<'static> = c("121160", "Oxygen Administration Rate");
pub const OxygenConsumption: Code<'static> = c("122239", "Oxygen Consumption");
pub const OxygenExtractionFraction: Code<'static> = c("126392", "Oxygen Extraction Fraction");
pub const PAPLeakPressure: Code<'static> = c("130423", "PAP Leak Pressure");
pub const PAPPressure: Code<'static> = c("130422", "PAP Pressure");
pub const PAPTidalVolume: Code<'static> = c("130424", "PAP Tidal Volume");
pub const PAS: Code<'static> = c("113237", "PAS");
pub const PCIDuringThisProcedure: Code<'static> = c("122129", "PCI during this procedure");
pub const PEARLDGS: Code<'static> = c("111877", "PEARL-DGS");
pub const PETImageReconstruction: Code<'static> = c("110017", "PET Image Reconstruction");
pub const PETMeasurementReport: Code<'static> = c("126003", "PET Measurement Report");
pub const PETMyocardialPerfusionRestAndStress: Code<'static> = c("122793", "PET Myocardial Perfusion, Rest and Stress");
pub const PETMyocardialPerfusionRestOnly: Code<'static> = c("122791", "PET Myocardial Perfusion, Rest only");
pub const PETMyocardialPerfusionStressOnly: Code<'static> = c("122792", "PET Myocardial Perfusion, Stress only");
pub const PETMyocardialViabilityRestAndStress: Code<'static> = c("122797", "PET Myocardial Viability, Rest and Stress");
pub const PETMyocardialViabilityRestOnly: Code<'static> = c("122795", "PET Myocardial Viability, Rest only");
pub const PETMyocardialViabilityStressOnly: Code<'static> = c("122796", "PET Myocardial Viability, Stress only");
pub const PETRadionuclideIncubationTime: Code<'static> = c("126203", "PET Radionuclide Incubation Time");
pub const PGreaterThan5Percent: Code<'static> = c("111935", "p>5%");
pub const PIRADS20ProstateMRIAcquisitionRequirements: Code<'static> = c("130607", "PI-RADS 2.0 prostate MRI acquisition requirements");
pub const PIRADS21ProstateMRIAcquisitionRequirements: Code<'static> = c("130608", "PI-RADS 2.1 prostate MRI acquisition requirements");
pub const PIRADSDCELesionAssessment: Code<'static> = c("130563", "PI-RADS DCE Lesion Assessment");
pub const PIRADSDWILesionAssessment: Code<'static> = c("130562", "PI-RADS DWI Lesion Assessment");
pub const PIRADSLocalizedAbnormalityAssessment: Code<'static> = c("130560", "PI-RADS Localized Abnormality Assessment");
pub const PIRADST2WILesionAssessment: Code<'static> = c("130561", "PI-RADS T2WI Lesion Assessment");
pub const PIRADSV20: Code<'static> = c("130564", "PI-RADS v2.0");
pub const PIRADSV21: Code<'static> = c("130565", "PI-RADS v2.1");
pub const PISARadius: Code<'static> = c("125331", "PISA Radius");
pub const PLessThan05Percent: Code<'static> = c("111939", "p<0.5%");
pub const PLessThan1Percent: Code<'static> = c("111938", "p<1%");
pub const PLessThan2Percent: Code<'static> = c("111937", "p<2%");
pub const PLessThan5Percent: Code<'static> = c("111936", "p<5%");
pub const PO10: Code<'static> = c("130715", "PO10");
pub const PO9: Code<'static> = c("130714", "PO9");
pub const PSMA1007F18: Code<'static> = c("126758", "PSMA-1007 F^18^");
pub const PSMA617Ga68: Code<'static> = c("126759", "PSMA-617 Ga^68^");
pub const PTVNodal: Code<'static> = c("130053", "PTV Nodal");
pub const PTVPrimary: Code<'static> = c("130054", "PTV Primary");
pub const PVVTIForward: Code<'static> = c("131050", "PV VTI Forward");
pub const PVVTIRPerVTIFRatio: Code<'static> = c("131052", "PV VTIR/VTIF ratio");
pub const PVVTIReverse: Code<'static> = c("131051", "PV VTI Reverse");
pub const PWave: Code<'static> = c("109041", "P wave");
pub const Paced: Code<'static> = c("109083", "Paced");
#[deprecated = "retired in PS3.16"]
pub const PacedStressTest: Code<'static> = c("121438", "Paced stress test");
#[deprecated = "retired in PS3.16"]
pub const Pacemaker: Code<'static> = c("111171", "Pacemaker");
pub const PacingElectricalStimulusVoltage: Code<'static> = c("109009", "Pacing (electrical) stimulus, voltage");
pub const PackageInsert: Code<'static> = c("113528", "Package Insert");
#[deprecated = "retired in PS3.16"]
pub const Paddle: Code<'static> = c("111172", "Paddle");
pub const PageSentTo: Code<'static> = c("122043", "Page Sent To");
pub const PalpationGuided: Code<'static> = c("111489", "Palpation guided");
pub const PancreaticStent: Code<'static> = c("112176", "Pancreatic stent");
pub const Panitumumab89Zr: Code<'static> = c("126736", "Panitumumab ^89^Zr");
pub const PanoramicXRay: Code<'static> = c("PX", "Panoramic X-Ray");
pub const PaperBasedBedding: Code<'static> = c("127233", "Paper-based bedding");
pub const PaperDigitized: Code<'static> = c("110023", "Paper Digitized");
pub const PaperDocument: Code<'static> = c("110038", "Paper Document");
#[deprecated = "retired in PS3.16"]
pub const PapillaryCarcinomaInSitu: Code<'static> = c("111336", "Papillary carcinoma in-situ");
pub const PapillaryMuscleExcluded: Code<'static> = c("122620", "Papillary Muscle Excluded");
pub const PapillaryMuscleIncluded: Code<'static> = c("122621", "Papillary Muscle Included");
pub const PapillaryMuscleIncludedPerExcluded: Code<'static> = c("122670", "Papillary Muscle Included/Excluded");
pub const Parallel: Code<'static> = c("111355", "Parallel");
pub const ParallelGrid: Code<'static> = c("111644", "Parallel grid");
pub const ParallelImaging: Code<'static> = c("113228", "Parallel Imaging");
pub const Parallelepiped: Code<'static> = c("122566", "Parallelepiped");
pub const ParaspinalLine: Code<'static> = c("112091", "Paraspinal line");
pub const ParenchymalBand: Code<'static> = c("112112", "Parenchymal band");
pub const ParentSpecimenIdentifier: Code<'static> = c("111705", "Parent Specimen Identifier");
pub const ParentSpecimenType: Code<'static> = c("111707", "Parent specimen type");
pub const ParentVesselFinding: Code<'static> = c("122686", "Parent Vessel Finding");
pub const PartialFlatteningFilterBeam: Code<'static> = c("130357", "Partial Flattening Filter Beam");
pub const PartialRenderingForPresentation: Code<'static> = c("121333", "Partial Rendering for Presentation");
pub const PartialReport: Code<'static> = c("121363", "Partial report");
pub const PartiallyEncapsulated: Code<'static> = c("130593", "Partially encapsulated");
pub const PartiallySucceeded: Code<'static> = c("111223", "Partially Succeeded");
pub const ParticleDisplacementMethod: Code<'static> = c("130756", "Particle Displacement Method");
pub const ParticleVelocityMethod: Code<'static> = c("130757", "Particle Velocity Method");
pub const Pasquini2007: Code<'static> = c("131535", "Pasquini 2007");
pub const PatencyTestInjection: Code<'static> = c("130247", "Patency Test Injection");
pub const Path: Code<'static> = c("121055", "Path");
pub const PathLength: Code<'static> = c("121211", "Path length");
pub const PathVertex: Code<'static> = c("121230", "Path Vertex");
#[deprecated = "retired in PS3.16"]
pub const Path_121210: Code<'static> = c("121210", "Path");
pub const Pathology: Code<'static> = c("111042", "Pathology");
pub const PathologyResults: Code<'static> = c("111468", "Pathology Results");
pub const Patient: Code<'static> = c("121025", "Patient");
pub const PatientAPDimension: Code<'static> = c("128408", "Patient AP Dimension");
pub const PatientAdmittedToProcedureRoom: Code<'static> = c("122002", "Patient admitted to procedure room");
pub const PatientAdvanceDirectiveGiven: Code<'static> = c("122005", "Patient advance directive given");
pub const PatientAlert: Code<'static> = c("122025", "Patient alert");
pub const PatientAlignmentProcedure: Code<'static> = c("130638", "Patient Alignment Procedure");
pub const PatientAllergicToMediaPerContrast: Code<'static> = c("110503", "Patient allergic to media/contrast");
pub const PatientAnatomyModel: Code<'static> = c("130067", "Patient Anatomy Model");
pub const PatientAsleep: Code<'static> = c("122028", "Patient asleep");
pub const PatientAssessmentPerformed: Code<'static> = c("121165", "Patient Assessment Performed");
pub const PatientAssistedToTable: Code<'static> = c("122007", "Patient assisted to table");
pub const PatientAttachedDoseControlObject: Code<'static> = c("130405", "Patient-Attached Dose Control Object");
pub const PatientAttenuationCharacteristics: Code<'static> = c("130529", "Patient Attenuation Characteristics");
pub const PatientCalledToProcedureRoom: Code<'static> = c("122001", "Patient called to procedure room");
pub const PatientCharacteristics: Code<'static> = c("121118", "Patient Characteristics");
pub const PatientClearanceInterlock: Code<'static> = c("130741", "Patient Clearance Interlock");
pub const PatientConditionPreventedContinuing: Code<'static> = c("110515", "Patient condition prevented continuing");
pub const PatientConnectedToContinuousMonitoring: Code<'static> = c("122009", "Patient connected to continuous monitoring");
pub const PatientConsciousness: Code<'static> = c("130865", "Patient Consciousness");
pub const PatientCoordinateSystemOrigin: Code<'static> = c("130540", "Patient Coordinate System Origin");
pub const PatientCoughed: Code<'static> = c("122031", "Patient coughed");
pub const PatientDataUsedDuringPlanning: Code<'static> = c("112361", "Patient Data Used During Planning");
pub const PatientDecisionToTerminateTreatment: Code<'static> = c("130451", "Patient decision to terminate treatment");
pub const PatientDidNotArrive: Code<'static> = c("110507", "Patient did not arrive");
pub const PatientDied: Code<'static> = c("110504", "Patient died");
pub const PatientDischargedFromDepartment: Code<'static> = c("122037", "Patient discharged from department");
pub const PatientDisconnectedFromContinuousMonitoring: Code<'static> = c("122032", "Patient disconnected from continuous monitoring");
pub const PatientDisoriented: Code<'static> = c("122020", "Patient disoriented");
pub const PatientDistractionDevice: Code<'static> = c("130651", "Patient Distraction Device");
pub const PatientElevation: Code<'static> = c("130412", "Patient elevation");
pub const PatientEquivalentThickness: Code<'static> = c("111638", "Patient Equivalent Thickness");
pub const PatientExposureToIonizingRadiation: Code<'static> = c("121290", "Patient exposure to ionizing radiation");
pub const PatientFixationProcedure: Code<'static> = c("130637", "Patient Fixation Procedure");
pub const PatientGivenPostProcedureInstruction: Code<'static> = c("122036", "Patient given post-procedure instruction");
pub const PatientGivenPreProcedureInstruction: Code<'static> = c("122003", "Patient given pre-procedure instruction");
pub const PatientHasRespiratoryDifficulty: Code<'static> = c("122030", "Patient has respiratory difficulty");
pub const PatientImage: Code<'static> = c("112354", "Patient Image");
pub const PatientInformedConsentGiven: Code<'static> = c("122004", "Patient informed consent given");
pub const PatientLateralDimension: Code<'static> = c("128409", "Patient Lateral Dimension");
pub const PatientLocationFiducial: Code<'static> = c("128754", "Patient Location Fiducial");
pub const PatientModel: Code<'static> = c("113815", "Patient Model");
pub const PatientModelDemographics: Code<'static> = c("128427", "Patient Model Demographics");
pub const PatientModelRegistration: Code<'static> = c("128456", "Patient Model Registration");
pub const PatientModelType: Code<'static> = c("128417", "Patient Model Type");
pub const PatientMotion: Code<'static> = c("128553", "Patient motion");
pub const PatientMotionInterlock: Code<'static> = c("130479", "Patient Motion Interlock");
pub const PatientMotionManagementSetupProcedure: Code<'static> = c("130639", "Patient Motion Management Setup Procedure");
pub const PatientMovement: Code<'static> = c("110518", "Patient Movement");
pub const PatientOrientation: Code<'static> = c("113743", "Patient Orientation");
pub const PatientOrientationColumn: Code<'static> = c("111043", "Patient Orientation Column");
pub const PatientOrientationModifier: Code<'static> = c("113744", "Patient Orientation Modifier");
pub const PatientOrientationRow: Code<'static> = c("111044", "Patient Orientation Row");
pub const PatientPosition: Code<'static> = c("130410", "Patient position");
pub const PatientPositioningNote: Code<'static> = c("130026", "Patient Positioning Note");
pub const PatientPositioningProcedureNote: Code<'static> = c("130035", "Patient Positioning Procedure Note");
pub const PatientPregnant: Code<'static> = c("110508", "Patient pregnant");
pub const PatientPreppedAndDraped: Code<'static> = c("122008", "Patient prepped and draped");
#[deprecated = "retired in PS3.16"]
pub const PatientPresentation: Code<'static> = c("121110", "Patient Presentation");
pub const PatientPronouncedDead: Code<'static> = c("122038", "Patient pronounced dead");
pub const PatientRadiationDoseModel: Code<'static> = c("128500", "Patient Radiation Dose Model");
pub const PatientRadiationDoseModelData: Code<'static> = c("128425", "Patient Radiation Dose Model Data");
pub const PatientRadiationDoseModelReference: Code<'static> = c("128426", "Patient Radiation Dose Model Reference");
pub const PatientRadiationDoseReport: Code<'static> = c("128401", "Patient Radiation Dose Report");
pub const PatientRecord: Code<'static> = c("110110", "Patient Record");
pub const PatientRefusedToContinueProcedure: Code<'static> = c("110505", "Patient refused to continue procedure");
pub const PatientReportsChestPain: Code<'static> = c("122023", "Patient reports chest pain");
pub const PatientReportsDiscomfort: Code<'static> = c("122022", "Patient reports discomfort");
pub const PatientReportsNausea: Code<'static> = c("122021", "Patient reports nausea");
pub const PatientReportsNoPain: Code<'static> = c("122024", "Patient reports no pain");
pub const PatientRepositioned: Code<'static> = c("130455", "Patient Repositioned");
pub const PatientRestless: Code<'static> = c("122026", "Patient restless");
pub const PatientRotationLongitudinal: Code<'static> = c("130411", "Patient rotation longitudinal");
pub const PatientSedated: Code<'static> = c("122027", "Patient sedated");
pub const PatientSegmentedModel: Code<'static> = c("128494", "Patient Segmented Model");
pub const PatientSetupNote: Code<'static> = c("130028", "Patient Setup Note");
pub const PatientSetupPoint: Code<'static> = c("130069", "Patient Setup Point");
pub const PatientSetupVerificationInputUsed: Code<'static> = c("128217", "Patient Setup Verification Input Used");
pub const PatientSetupVerificationResult: Code<'static> = c("128191", "Patient Setup Verification Result");
pub const PatientShieldingProcedure: Code<'static> = c("130636", "Patient Shielding Procedure");
pub const PatientState: Code<'static> = c("109054", "Patient State");
pub const PatientStatusOrEvent: Code<'static> = c("121123", "Patient Status or Event");
pub const PatientSupport: Code<'static> = c("128492", "Patient Support");
pub const PatientSupportOrigin: Code<'static> = c("130538", "Patient Support Origin");
pub const PatientTableRelationship: Code<'static> = c("113745", "Patient Table Relationship");
pub const PatientTakenForTreatmentOrSurgery: Code<'static> = c("110506", "Patient taken for treatment or surgery");
pub const PatientTransferredFrom: Code<'static> = c("122128", "Patient Transferred From");
pub const PatientTransferredToCCU: Code<'static> = c("122012", "Patient transferred to CCU");
pub const PatientTransferredToHoldingArea: Code<'static> = c("122010", "Patient transferred to holding area");
pub const PatientTransferredToMorgue: Code<'static> = c("122039", "Patient transferred to morgue");
pub const PatientTransferredToSurgery: Code<'static> = c("122011", "Patient transferred to surgery");
pub const PatientUnresponsive: Code<'static> = c("122029", "Patient unresponsive");
pub const PatientWeightExceedsEquipmentLimit: Code<'static> = c("122764", "Patient weight exceeds equipment limit");
pub const PatternEvent: Code<'static> = c("130860", "Pattern Event");
pub const PatternProjection: Code<'static> = c("114204", "Pattern projection");
pub const PeakActivityWorkload: Code<'static> = c("122717", "Peak activity workload");
pub const PeakBloodPressure: Code<'static> = c("125329", "Peak Blood Pressure");
pub const PeakDoubleProduct: Code<'static> = c("122718", "Peak Double Product");
pub const PeakEjectionRate: Code<'static> = c("122616", "Peak Ejection Rate");
pub const PeakEjectionTime: Code<'static> = c("122617", "Peak Ejection Time");
pub const PeakFillingRate: Code<'static> = c("122618", "Peak Filling Rate");
pub const PeakFillingTime: Code<'static> = c("122619", "Peak Filling Time");
pub const PeakFlowRateInPhaseActivity: Code<'static> = c("130244", "Peak Flow Rate in Phase Activity");
pub const PeakLongitudinalLagrangianStrain: Code<'static> = c("130671", "Peak Longitudinal Lagrangian Strain");
pub const PeakOfThermalCardiacOutputBolus: Code<'static> = c("109028", "Peak of thermal cardiac output bolus");
pub const PeakPressureInPhaseActivity: Code<'static> = c("130245", "Peak Pressure in Phase Activity");
#[deprecated = "retired in PS3.16"]
pub const PeakStressState: Code<'static> = c("109095", "Peak stress state");
pub const PeakTissueVelocity: Code<'static> = c("125330", "Peak Tissue Velocity");
pub const PeakValueWithinROI: Code<'static> = c("126031", "Peak Value Within ROI");
pub const PeakVelocityIndex: Code<'static> = c("131013", "Peak Velocity Index");
pub const PectoralMuscleOutline: Code<'static> = c("111045", "Pectoral Muscle Outline");
pub const PediatricCardiacUltrasoundReport: Code<'static> = c("125195", "Pediatric Cardiac Ultrasound Report");
pub const PediatricImagingSpecialty: Code<'static> = c("128016", "Pediatric Imaging Specialty");
pub const PediatricImagingSubjectMatter: Code<'static> = c("128737", "Pediatric imaging subject matter");
pub const PediatricRadiology: Code<'static> = c("128177", "Pediatric Radiology");
pub const Pegdinetanib89Zr: Code<'static> = c("126728", "Pegdinetanib ^89^Zr");
pub const PelvisAndUterus: Code<'static> = c("125011", "Pelvis and Uterus");
pub const PencilBeamConvolution: Code<'static> = c("131321", "Pencil Beam Convolution");
pub const PercentFibroglandularTissue: Code<'static> = c("111046", "Percent Fibroglandular Tissue");
pub const PercentageOfExpectedBeamOnTimeOfRadiation: Code<'static> = c("130800", "Percentage of expected beam-on time of Radiation");
pub const PercentageOfVascularCalcification: Code<'static> = c("112198", "Percentage of Vascular Calcification");
pub const PercentileRankingOfMeasurement: Code<'static> = c("121415", "Percentile Ranking of measurement");
pub const PercutaneousCoronaryIntervention: Code<'static> = c("122061", "Percutaneous Coronary Intervention");
pub const PercutaneousEntryAction: Code<'static> = c("121156", "Percutaneous Entry Action");
pub const PercutaneousSiliconeInjection: Code<'static> = c("111483", "Percutaneous silicone injection");
pub const Perfluorocarbon: Code<'static> = c("130605", "Perfluorocarbon");
pub const PerformanceOfCTForDetectionOfPulmonaryEmbolismInAdults: Code<'static> = c("112185", "Performance of CT for Detection of Pulmonary Embolism in Adults");
pub const PerformanceOfHighResolutionCTOfTheLungsInAdults: Code<'static> = c("112186", "Performance of High-Resolution CT of the Lungs in Adults");
pub const PerformanceOfPediatricAndAdultChestRadiographyACR: Code<'static> = c("112035", "Performance of Pediatric and Adult Chest Radiography, ACR");
pub const PerformanceOfPediatricAndAdultThoracicCT: Code<'static> = c("112184", "Performance of Pediatric and Adult Thoracic CT");
pub const PerformedImagingAgentAdministration: Code<'static> = c("130227", "Performed Imaging Agent Administration");
pub const PerformedProcedureStep: Code<'static> = c("113016", "Performed Procedure Step");
pub const PerformedProcedureStepSOPClassUID: Code<'static> = c("121127", "Performed Procedure Step SOP Class UID");
pub const PerformedProcedureStepSOPInstanceUID: Code<'static> = c("121126", "Performed Procedure Step SOP Instance UID");
pub const Performing: Code<'static> = c("121094", "Performing");
pub const PerformingPhysician: Code<'static> = c("121114", "Performing Physician");
pub const Perfusion: Code<'static> = c("110831", "Perfusion");
pub const PerfusionAnalysisByArterialSpinLabelingMRTechnique: Code<'static> = c("126302", "Perfusion analysis by Arterial Spin Labeling MR technique");
pub const PerfusionAnalysisByIVIodinatedContrastCTTechnique: Code<'static> = c("126301", "Perfusion analysis by IV Iodinated Contrast CT technique");
pub const PerfusionAnalysisByStableXenonCTTechnique: Code<'static> = c("126300", "Perfusion analysis by Stable Xenon CT technique");
pub const PerfusionAnalysisBySusceptibilityMRTechnique: Code<'static> = c("126303", "Perfusion analysis by Susceptibility MR technique");
pub const PerfusionImageAnalysis: Code<'static> = c("129104", "Perfusion image analysis");
pub const PeriOperativePhotographicImaging: Code<'static> = c("112700", "Peri-operative Photographic Imaging");
pub const PeriProceduralMIOccurred: Code<'static> = c("122179", "Peri-procedural MI occurred");
pub const PerilesionalWhiteMatter: Code<'static> = c("110706", "Perilesional White Matter");
pub const PerimeterOutline: Code<'static> = c("121057", "Perimeter outline");
#[deprecated = "retired in PS3.16"]
pub const PerimeterOutline_121213: Code<'static> = c("121213", "Perimeter Outline");
pub const PeripheralDuctPapillomas: Code<'static> = c("111299", "Peripheral duct papillomas");
pub const PeripheralInferior: Code<'static> = c("111916", "Peripheral-inferior");
pub const PeripheralInferiorNasal: Code<'static> = c("111917", "Peripheral-inferior nasal");
pub const PeripheralInferiorTemporal: Code<'static> = c("111915", "Peripheral-inferior temporal");
pub const PeripheralNasal: Code<'static> = c("111918", "Peripheral-nasal");
pub const PeripheralSuperior: Code<'static> = c("111912", "Peripheral-superior");
pub const PeripheralSuperiorNasal: Code<'static> = c("111919", "Peripheral-superior nasal");
pub const PeripheralSuperiorTemporal: Code<'static> = c("111913", "Peripheral-superior temporal");
pub const PeripheralTemporal: Code<'static> = c("111914", "Peripheral-temporal");
pub const PerivascularAdiposeTissueFatAttenuationIndex: Code<'static> = c("130408", "Perivascular adipose tissue fat attenuation index");
pub const PersistentDelayedPhaseType1Curve: Code<'static> = c("130597", "Persistent delayed phase: Type 1 curve");
pub const Person: Code<'static> = c("121006", "Person");
pub const PersonAdministeringDrugPerContrast: Code<'static> = c("121152", "Person administering drug/contrast");
pub const PersonID: Code<'static> = c("113871", "Person ID");
pub const PersonIDIssuer: Code<'static> = c("113872", "Person ID Issuer");
pub const PersonName: Code<'static> = c("113870", "Person Name");
pub const PersonObserverName: Code<'static> = c("121008", "Person Observer Name");
pub const PersonObserversLoginName: Code<'static> = c("128774", "Person Observer's Login Name");
pub const PersonObserversOrganizationName: Code<'static> = c("121009", "Person Observer's Organization Name");
pub const PersonObserversRoleInTheOrganization: Code<'static> = c("121010", "Person Observer's Role in the Organization");
pub const PersonObserversRoleInThisProcedure: Code<'static> = c("121011", "Person Observer's Role in this Procedure");
pub const PersonRoleInOrganization: Code<'static> = c("113874", "Person Role in Organization");
pub const PersonRoleInProcedure: Code<'static> = c("113875", "Person Role in Procedure");
pub const PersonalBreastCancerHistory: Code<'static> = c("111550", "Personal breast cancer history");
pub const PersonalContent: Code<'static> = c("131392", "Personal Content");
pub const PersonalHistoryOfBreastCancerWithBreastConservationTherapy: Code<'static> = c("111421", "Personal history of breast cancer with breast conservation therapy");
pub const PersonalHistoryOfBreastCancerWithMastectomy: Code<'static> = c("111124", "Personal history of breast cancer with mastectomy");
pub const PersonnelArrived: Code<'static> = c("122041", "Personnel Arrived");
pub const PersonnelDeparted: Code<'static> = c("122042", "Personnel Departed");
#[deprecated = "retired in PS3.16"]
pub const Phantom: Code<'static> = c("113681", "Phantom");
pub const PhantomTumorPseudotumor: Code<'static> = c("112123", "Phantom tumor (pseudotumor)");
#[deprecated = "retired in PS3.16"]
pub const PharmacologicAndExerciseStressTest: Code<'static> = c("121437", "Pharmacologic and exercise stress test");
pub const PharmacologicalStressAgentDose: Code<'static> = c("122715", "Pharmacological Stress Agent Dose");
pub const PharmacologicalStressAgentDoseRate: Code<'static> = c("122705", "Pharmacological Stress Agent Dose Rate");
pub const PhaseContrastIllumination: Code<'static> = c("111747", "Phase contrast illumination");
pub const PhaseOfAnimalHandling: Code<'static> = c("127006", "Phase of animal handling");
pub const PhasedBeamSteering: Code<'static> = c("125259", "Phased beam steering");
pub const Phonocardiogram: Code<'static> = c("109114", "Phonocardiogram");
pub const Photoacoustic: Code<'static> = c("PA", "Photoacoustic");
pub const PhotoelectricEffect: Code<'static> = c("131569", "Photoelectric Effect");
pub const PhotogrammetricImaging3DCAMModel: Code<'static> = c("129020", "Photogrammetric Imaging 3D CAM model");
pub const PhotonEnergy: Code<'static> = c("110857", "Photon Energy");
pub const PhotonFixedAperture: Code<'static> = c("130344", "Photon Fixed Aperture");
pub const PhotopicLight: Code<'static> = c("111629", "Photopic light");
pub const PhysicalCompensator: Code<'static> = c("130340", "Physical Compensator");
pub const PhysicalExaminationResults: Code<'static> = c("111423", "Physical Examination Results");
pub const PhysicalForce: Code<'static> = c("109133", "Physical force");
#[deprecated = "retired in PS3.16"]
pub const Physician: Code<'static> = c("121081", "Physician");
pub const PhysicianDecisionToTerminateTreatment: Code<'static> = c("130452", "Physician decision to terminate treatment");
pub const PhysicianNote: Code<'static> = c("121173", "Physician Note");
pub const PhysicianReviewInputUsed: Code<'static> = c("128213", "Physician Review Input Used");
pub const PhysicistReviewInputUsed: Code<'static> = c("128214", "Physicist Review Input Used");
pub const PhysicsAssistant: Code<'static> = c("128678", "Physics Assistant");
pub const PhysiologicalAudioSignal: Code<'static> = c("109115", "Physiological audio signal");
pub const PhysiologicalChallenges: Code<'static> = c("109059", "Physiological challenges");
pub const PiezocompositeTransducer: Code<'static> = c("130815", "Piezocomposite Transducer");
pub const PinatuzumabVedotin89Zr: Code<'static> = c("126725", "Pinatuzumab vedotin ^89^Zr");
pub const PineChipBedding: Code<'static> = c("127234", "Pine chip bedding");
pub const PineShavingBedding: Code<'static> = c("127235", "Pine shaving bedding");
pub const Pinning: Code<'static> = c("112318", "Pinning");
pub const PitchFactor: Code<'static> = c("113828", "Pitch Factor");
pub const PittsburghCompoundBC11: Code<'static> = c("126500", "Pittsburgh compound B C^11^");
pub const PixelByPixelAddition: Code<'static> = c("113042", "Pixel by pixel addition");
pub const PixelByPixelDivision: Code<'static> = c("113046", "Pixel by pixel division");
pub const PixelByPixelMask: Code<'static> = c("113047", "Pixel by pixel mask");
pub const PixelByPixelMaximum: Code<'static> = c("113048", "Pixel by pixel Maximum");
pub const PixelByPixelMean: Code<'static> = c("113049", "Pixel by pixel mean");
pub const PixelByPixelMinimum: Code<'static> = c("113051", "Pixel by pixel Minimum");
pub const PixelByPixelMultiplication: Code<'static> = c("113053", "Pixel by pixel multiplication");
pub const PixelByPixelSubtraction: Code<'static> = c("113062", "Pixel by pixel subtraction");
pub const PixelDataColumns: Code<'static> = c("110911", "Pixel Data Columns");
pub const PixelDataRows: Code<'static> = c("110910", "Pixel Data Rows");
pub const PixelFaultsEvaluation: Code<'static> = c("109706", "Pixel faults evaluation");
pub const PixelatedSpiculations: Code<'static> = c("113666", "Pixelated spiculations");
pub const PlacerNumber: Code<'static> = c("121020", "Placer Number");
pub const Plan: Code<'static> = c("PLAN", "Plan");
pub const PlanComparisonInputUsed: Code<'static> = c("128220", "Plan Comparison Input Used");
pub const PlanMeetsPrescription: Code<'static> = c("131315", "Plan meets prescription");
pub const PlanQAPassed: Code<'static> = c("131316", "Plan QA passed");
pub const PlanQualityAssuranceInputUsed: Code<'static> = c("128215", "Plan Quality Assurance Input Used");
pub const PlanarMatrixUltrasoundTransducerGeometry: Code<'static> = c("130808", "Planar matrix ultrasound transducer geometry");
pub const PlaneA: Code<'static> = c("113620", "Plane A");
pub const PlaneB: Code<'static> = c("113621", "Plane B");
pub const PlaneScanPattern: Code<'static> = c("125241", "Plane scan pattern");
pub const PlaneThroughAnteriorExtent: Code<'static> = c("128128", "Plane through Anterior Extent");
pub const PlaneThroughCenter: Code<'static> = c("128130", "Plane through Center");
pub const PlaneThroughDistalExtent: Code<'static> = c("128123", "Plane through Distal Extent");
pub const PlaneThroughInferiorExtent: Code<'static> = c("128121", "Plane through Inferior Extent");
pub const PlaneThroughLateralExtent: Code<'static> = c("128125", "Plane through Lateral Extent");
pub const PlaneThroughLeftmostExtent: Code<'static> = c("128126", "Plane through Leftmost Extent");
pub const PlaneThroughMedialExtent: Code<'static> = c("128124", "Plane through Medial Extent");
pub const PlaneThroughPosteriorExtent: Code<'static> = c("128129", "Plane through Posterior Extent");
pub const PlaneThroughProximalExtent: Code<'static> = c("128122", "Plane through Proximal Extent");
pub const PlaneThroughRightmostExtent: Code<'static> = c("128127", "Plane through Rightmost Extent");
pub const PlaneThroughSuperiorExtent: Code<'static> = c("128120", "Plane through Superior Extent");
pub const Planimetry: Code<'static> = c("125220", "Planimetry");
pub const PlannedImagingAgentAdministration: Code<'static> = c("130226", "Planned Imaging Agent Administration");
pub const PlannedImagingAgentAdministrationProcedureReport: Code<'static> = c("130182", "Planned Imaging Agent Administration Procedure Report");
pub const PlannedImagingAgentAdministrationSOPInstance: Code<'static> = c("130236", "Planned Imaging Agent Administration SOP Instance");
pub const PlanningImagingNote: Code<'static> = c("130030", "Planning Imaging Note");
pub const PlanningInformationForIntraoperativeUsage: Code<'static> = c("112367", "Planning Information for Intraoperative Usage");
pub const PlanningIntent: Code<'static> = c("129013", "Planning Intent");
pub const PlanningMethod: Code<'static> = c("112375", "Planning Method");
pub const PlanningOrganAtRiskVolume: Code<'static> = c("130057", "Planning Organ At Risk Volume");
pub const PlaqueArea: Code<'static> = c("122542", "Plaque Area");
pub const PlaqueBurden: Code<'static> = c("122354", "Plaque Burden");
pub const PlaquePlusMediaCrossSectionalArea: Code<'static> = c("122334", "Plaque plus Media Cross-Sectional Area");
pub const PlaquePlusMediaEccentricityIndex: Code<'static> = c("122344", "Plaque plus Media Eccentricity Index");
pub const PlaquePlusMediaThickness: Code<'static> = c("122331", "Plaque Plus Media Thickness");
pub const PlaqueRupture: Code<'static> = c("122363", "Plaque Rupture");
pub const PlasmaDisplay: Code<'static> = c("109993", "Plasma Display");
pub const PlateauDelayedPhaseType2Curve: Code<'static> = c("130598", "Plateau delayed phase: Type 2 curve");
pub const Pleonemia: Code<'static> = c("112081", "Pleonemia");
pub const PointCloudAlgorithmic: Code<'static> = c("114208", "Point Cloud Algorithmic");
pub const PointShearWaveElastography: Code<'static> = c("130616", "Point Shear Wave Elastography");
pub const PoiseuilleResistance: Code<'static> = c("122549", "Poiseuille Resistance");
pub const PolarToRectangularScanConversion: Code<'static> = c("113093", "Polar to Rectangular Scan Conversion");
#[deprecated = "retired in PS3.16"]
pub const PolarizingFilter: Code<'static> = c("111607", "Polarizing filter");
pub const PolatuzumabVedotin89Zr: Code<'static> = c("126726", "Polatuzumab vedotin ^89^Zr");
pub const PolypStalkWidth: Code<'static> = c("112232", "Polyp stalk width");
pub const PolyurethaneImplant: Code<'static> = c("111482", "Polyurethane implant");
pub const PoorContrastTiming: Code<'static> = c("130579", "Poor contrast timing");
pub const PoorFOVSelection: Code<'static> = c("130577", "Poor FOV selection");
pub const PoorImageQuality: Code<'static> = c("122742", "Poor image quality");
pub const PoorSNR: Code<'static> = c("130578", "Poor SNR");
pub const PoorlyDemarcated: Code<'static> = c("112141", "Poorly demarcated");
pub const PopulationAveragedAIF: Code<'static> = c("126361", "Population Averaged AIF");
pub const PopulationDescription: Code<'static> = c("121405", "Population description");
pub const PopulationIndex: Code<'static> = c("121418", "Population Index");
pub const PortableMediaImporterEquipment: Code<'static> = c("MEDIM", "Portable Media Importer Equipment");
pub const Portacath: Code<'static> = c("112172", "Portacath");
pub const PositionFrameOfReference: Code<'static> = c("111708", "Position Frame of Reference");
pub const PositionOfDistalBorder: Code<'static> = c("122529", "Position of Distal Border");
pub const PositionOfProximalBorder: Code<'static> = c("122528", "Position of Proximal Border");
pub const PositionReferenceIndicator: Code<'static> = c("127451", "Position reference indicator");
pub const PositionSensor: Code<'static> = c("POS", "Position Sensor");
pub const PositionerIsocenterDetectorRotationAngle: Code<'static> = c("128759", "Positioner Isocenter Detector Rotation Angle");
pub const PositionerIsocenterDetectorRotationEndAngle: Code<'static> = c("128762", "Positioner Isocenter Detector Rotation End Angle");
pub const PositionerIsocenterPrimaryAngle: Code<'static> = c("128757", "Positioner Isocenter Primary Angle");
pub const PositionerIsocenterPrimaryEndAngle: Code<'static> = c("128760", "Positioner Isocenter Primary End Angle");
pub const PositionerIsocenterSecondaryAngle: Code<'static> = c("128758", "Positioner Isocenter Secondary Angle");
pub const PositionerIsocenterSecondaryEndAngle: Code<'static> = c("128761", "Positioner Isocenter Secondary End Angle");
pub const PositionerPrimaryAngle: Code<'static> = c("112011", "Positioner Primary Angle");
pub const PositionerPrimaryEndAngle: Code<'static> = c("113739", "Positioner Primary End Angle");
pub const PositionerSecondaryAngle: Code<'static> = c("112012", "Positioner Secondary Angle");
pub const PositionerSecondaryEndAngle: Code<'static> = c("113740", "Positioner Secondary End Angle");
pub const Positioning: Code<'static> = c("111209", "Positioning");
pub const PositiveDCE: Code<'static> = c("130600", "Positive DCE");
pub const PositiveInfinity: Code<'static> = c("114002", "Positive Infinity");
pub const PositronEmissionTomography: Code<'static> = c("PT", "Positron emission tomography");
pub const PostAdministrationMeasuredActivity: Code<'static> = c("113509", "Post-Administration Measured Activity");
pub const PostCoordinatedMeasurements: Code<'static> = c("125302", "Post-coordinated Measurements");
pub const PostHyperventilation: Code<'static> = c("130415", "Post-hyperventilation");
pub const PostIntervention: Code<'static> = c("109124", "Post-intervention");
#[deprecated = "retired in PS3.16"]
pub const PostInterventionStenosisMeasurement: Code<'static> = c("122108", "Post-Intervention Stenosis Measurement");
pub const PostInterventionTIMIFlow: Code<'static> = c("122110", "Post-Intervention TIMI Flow");
pub const PostLeftAtriumSpaceIndex: Code<'static> = c("131004", "Post-Left Atrium Space Index");
pub const PostMenopausalPatient: Code<'static> = c("111554", "Post menopausal patient");
pub const PostProceduralStructuralHeartMeasurementReport: Code<'static> = c("131092", "Post-procedural Structural Heart Measurement Report");
pub const PostProcedureMammogramsForMarkerPlacement: Code<'static> = c("111120", "Post Procedure Mammograms for Marker Placement");
pub const PostReductionMammoplasty: Code<'static> = c("111291", "Post reduction mammoplasty");
pub const PostVoiding: Code<'static> = c("109135", "Post voiding");
pub const PosteriorAcousticFeatures: Code<'static> = c("111366", "Posterior acoustic features");
pub const PosteriorCorneaSurfaceMeasurementMethod: Code<'static> = c("111759", "Posterior Cornea Surface Measurement Method");
pub const PosteriorEnhancement: Code<'static> = c("111368", "Posterior enhancement");
pub const PosteriorFirst: Code<'static> = c("126832", "posterior first");
pub const PosteriorJunctionLine: Code<'static> = c("112089", "Posterior junction line");
pub const PosteriorLens: Code<'static> = c("111779", "Posterior Lens");
pub const PosteriorMitralValveLeafletArea: Code<'static> = c("131203", "Posterior mitral valve leaflet area");
pub const PosteriorMitralValveLeafletLength: Code<'static> = c("131204", "Posterior mitral valve leaflet length");
pub const PosteriorMitralValveP1LeafletScallopLength: Code<'static> = c("131205", "Posterior mitral valve P1 leaflet scallop length");
pub const PosteriorMitralValveP2LeafletScallopLength: Code<'static> = c("131206", "Posterior mitral valve P2 leaflet scallop length");
pub const PosteriorMitralValveP3LeafletScallopLength: Code<'static> = c("131207", "Posterior mitral valve P3 leaflet scallop length");
pub const PosteriorPole8x8Grid: Code<'static> = c("131307", "Posterior pole 8x8 grid");
pub const PosteriorShadowing: Code<'static> = c("111369", "Posterior shadowing");
pub const PosteriorToAnterior: Code<'static> = c("110871", "Posterior To Anterior");
pub const PosteriorTrachealStripe: Code<'static> = c("112092", "Posterior tracheal stripe");
pub const PosteromedialPapillaryMuscleToTheRightTrigone: Code<'static> = c("131193", "Posteromedial papillary muscle to the right trigone");
pub const Posttreatment: Code<'static> = c("126074", "Posttreatment");
#[deprecated = "retired in PS3.16"]
pub const PowerDoppler: Code<'static> = c("125230", "Power Doppler");
pub const PreAdministrationMeasuredActivity: Code<'static> = c("113508", "Pre-Administration Measured Activity");
pub const PreBiopsyLocalizationOfProstateLesion: Code<'static> = c("130588", "Pre-biopsy localization of prostate lesion");
pub const PreCoordinatedMeasurements: Code<'static> = c("125301", "Pre-coordinated Measurements");
pub const PreEjectionPeriod: Code<'static> = c("125321", "Pre-ejection Period");
pub const PreHeatedPad: Code<'static> = c("127254", "Pre-heated pad");
pub const PreIntervention: Code<'static> = c("109123", "Pre-intervention");
pub const PreMedication: Code<'static> = c("123012", "Pre-Medication");
pub const PrePectoralImplant: Code<'static> = c("111485", "Pre-pectoral implant");
pub const PrePlanningResult: Code<'static> = c("128184", "Pre-Planning Result");
pub const PreProceduralStructuralHeartMeasurementReport: Code<'static> = c("131090", "Pre-procedural Structural Heart Measurement Report");
pub const PreProcedureLog: Code<'static> = c("122072", "Pre-procedure log");
pub const PreSurgicalAnatomy: Code<'static> = c("130833", "Pre-surgical anatomy");
pub const PreamplifierConnected: Code<'static> = c("130889", "Preamplifier connected");
pub const PreamplifierDisconnected: Code<'static> = c("130890", "Preamplifier disconnected");
pub const PreclinicalSmallAnimalImagingAcquisitionContext: Code<'static> = c("127001", "Preclinical Small Animal Imaging Acquisition Context");
pub const PredecessorContainingGroupOfImagingSubjects: Code<'static> = c("113130", "Predecessor containing group of imaging subjects");
#[deprecated = "retired in PS3.16"]
pub const PregnancyStatus: Code<'static> = c("111532", "Pregnancy Status");
pub const PreliminaryReport: Code<'static> = c("121362", "Preliminary report");
pub const PreparationForImaging: Code<'static> = c("127104", "Preparation for imaging");
pub const PreparationRoom: Code<'static> = c("127371", "Preparation room");
pub const PrescriptionIdentifier: Code<'static> = c("113516", "Prescription Identifier");
pub const PrescriptionRadiationDose: Code<'static> = c("130009", "Prescription Radiation Dose");
pub const PrescriptionVisualAcuity: Code<'static> = c("111687", "Prescription Visual Acuity");
pub const PresenceOfProperty: Code<'static> = c("121052", "Presence of property");
#[deprecated = "retired in PS3.16"]
pub const PresenceUndetermined: Code<'static> = c("121059", "Presence Undetermined");
#[deprecated = "retired in PS3.16"]
pub const Present: Code<'static> = c("121053", "Present");
pub const PresentationOptionalRenderingDeviceMayPresent: Code<'static> = c("111151", "Presentation Optional: Rendering device may present");
pub const PresentationRequiredRenderingDeviceIsExpectedToPresent: Code<'static> = c("111150", "Presentation Required: Rendering device is expected to present");
pub const PresentationState: Code<'static> = c("PR", "Presentation State");
pub const PressureAboveAdjustmentLimit: Code<'static> = c("130151", "Pressure above adjustment limit");
pub const PressureAboveWarningLimit: Code<'static> = c("130150", "Pressure above warning limit");
pub const PressureAtDpPerDtMax: Code<'static> = c("122199", "Pressure at dp/dt max");
pub const PressureDropAtSFR: Code<'static> = c("122551", "Pressure Drop at SFR");
pub const PressureLimit: Code<'static> = c("130193", "Pressure Limit");
pub const PressureVsTime: Code<'static> = c("130230", "Pressure vs Time");
pub const PreviousFetalMalformationPerSyndrome: Code<'static> = c("111570", "Previous fetal malformation/syndrome");
pub const PreviousFinding: Code<'static> = c("121069", "Previous Finding");
#[deprecated = "retired in PS3.16"]
pub const PreviousFindings: Code<'static> = c("121068", "Previous Findings");
pub const PreviousLBWOrIUGRBirth: Code<'static> = c("111569", "Previous LBW or IUGR birth");
pub const PreviousProcedure: Code<'static> = c("111531", "Previous Procedure");
pub const PreviousRHNegativeOrBloodDyscrasiaAtBirth: Code<'static> = c("111571", "Previous RH negative or blood dyscrasia at birth");
pub const PreviousReports: Code<'static> = c("111549", "Previous Reports");
pub const PreviousTreatmentNote: Code<'static> = c("130029", "Previous Treatment Note");
pub const PrimaryComplex: Code<'static> = c("112059", "Primary complex");
pub const PrimaryFluenceMonitoringSystemInterlock: Code<'static> = c("130475", "Primary Fluence Monitoring System Interlock");
pub const PrimaryFollicle: Code<'static> = c("130912", "Primary Follicle");
pub const PrimaryInterventionDevice: Code<'static> = c("122111", "Primary Intervention Device");
pub const PrimaryLevel: Code<'static> = c("128725", "Primary level");
pub const PrimordialFollicle: Code<'static> = c("130913", "Primordial Follicle");
pub const Print: Code<'static> = c("110008", "Print");
#[deprecated = "retired in PS3.16"]
pub const PriorProcedureDescriptions: Code<'static> = c("121066", "Prior Procedure Descriptions");
pub const PriorReportForCurrentPatient: Code<'static> = c("122075", "Prior report for current patient");
pub const PriorToVoiding: Code<'static> = c("109134", "Prior to voiding");
pub const Probabilistic: Code<'static> = c("113212", "Probabilistic");
pub const Probability: Code<'static> = c("122157", "Probability");
pub const ProbabilityOfCancer: Code<'static> = c("111047", "Probability of cancer");
pub const ProcedureActionDuration: Code<'static> = c("121128", "Procedure Action Duration");
pub const ProcedureActionID: Code<'static> = c("121124", "Procedure Action ID");
pub const ProcedureAnnotated: Code<'static> = c("130871", "Procedure annotated");
pub const ProcedureCharacteristics: Code<'static> = c("130530", "Procedure Characteristics");
pub const ProcedureCode: Code<'static> = c("121023", "Procedure Code");
pub const ProcedureDateTime: Code<'static> = c("122146", "Procedure DateTime");
pub const ProcedureDescription: Code<'static> = c("121065", "Procedure Description");
pub const ProcedureEnvironmentalCharacteristics: Code<'static> = c("122222", "Procedure Environmental Characteristics");
pub const ProcedureLogging: Code<'static> = c("LOG", "Procedure Logging");
pub const ProcedureModifier: Code<'static> = c("111464", "Procedure Modifier");
pub const ProcedureNote: Code<'static> = c("121174", "Procedure Note");
pub const ProcedureRecord: Code<'static> = c("110111", "Procedure Record");
pub const ProcedureReported: Code<'static> = c("121058", "Procedure reported");
pub const ProcedureResult: Code<'static> = c("122177", "Procedure Result");
pub const ProcedureStepNumber: Code<'static> = c("109060", "Procedure Step Number");
pub const ProcedureStepToThisPoint: Code<'static> = c("113970", "Procedure Step To This Point");
pub const ProcedureStudyComponentUID: Code<'static> = c("121019", "Procedure Study Component UID");
pub const ProcedureStudyInstanceUID: Code<'static> = c("121018", "Procedure Study Instance UID");
pub const ProcedureTimeBase: Code<'static> = c("122701", "Procedure time base");
pub const ProcessedKeyObjectSelection: Code<'static> = c("130753", "Processed Key Object Selection");
pub const ProcessingEquipment: Code<'static> = c("109102", "Processing Equipment");
pub const ProcessingStepDescription: Code<'static> = c("111703", "Processing step description");
pub const ProcessingType: Code<'static> = c("111701", "Processing type");
pub const Profusion: Code<'static> = c("112151", "Profusion");
pub const ProgesteroneReceptor: Code<'static> = c("111476", "Progesterone receptor");
pub const ProgrammableInjectorDevice: Code<'static> = c("130218", "Programmable Injector Device");
pub const ProgrammedHoldStarted: Code<'static> = c("130266", "Programmed hold started");
pub const ProjectionEponymousName: Code<'static> = c("113946", "Projection Eponymous Name");
pub const ProjectionSpaceMAR: Code<'static> = c("131542", "Projection-Space MAR");
pub const ProjectionXRay: Code<'static> = c("113704", "Projection X-Ray");
pub const ProliferativeActivity: Code<'static> = c("126036", "Proliferative Activity");
pub const ProspectiveGating: Code<'static> = c("109081", "Prospective gating");
pub const ProstateImagingFindings: Code<'static> = c("130553", "Prostate Imaging Findings");
pub const ProstateMRIRelevantProcedureInformation: Code<'static> = c("130552", "Prostate MRI relevant procedure information");
pub const ProstateRelationalMeasurements: Code<'static> = c("130556", "Prostate relational measurements");
pub const ProstheticFabrication: Code<'static> = c("129015", "Prosthetic Fabrication");
pub const ProstheticShaftAugment: Code<'static> = c("112316", "Prosthetic Shaft Augment");
pub const ProtocolNotFollowed: Code<'static> = c("130570", "Protocol not followed");
pub const ProtocolStage: Code<'static> = c("109055", "Protocol Stage");
pub const ProtocolTimePointIdentifier: Code<'static> = c("126071", "Protocol Time Point Identifier");
pub const ProtonDensity: Code<'static> = c("113058", "Proton Density");
pub const ProtonDensityWeightedMRSignalIntensity: Code<'static> = c("110803", "Proton Density Weighted MR Signal Intensity");
pub const ProximalFindingSite: Code<'static> = c("121116", "Proximal Finding Site");
pub const ProximalIsovelocitySurfaceArea: Code<'static> = c("125216", "Proximal Isovelocity Surface Area");
pub const ProximalReference: Code<'static> = c("122380", "Proximal Reference");
pub const ProximalStentMargin: Code<'static> = c("122385", "Proximal Stent Margin");
pub const PseudoangiomatousStromalHyperplasia: Code<'static> = c("111292", "Pseudoangiomatous stromal hyperplasia");
pub const Pseudoplaque: Code<'static> = c("112068", "Pseudoplaque");
pub const PulmonaryArterialContentFCpa: Code<'static> = c("122188", "Pulmonary Arterial Content (FCpa)");
pub const PulmonaryFlow: Code<'static> = c("122161", "Pulmonary Flow");
pub const PulmonaryImagingSubjectMatter: Code<'static> = c("128729", "Pulmonary imaging subject matter");
pub const PulmonaryVenousContentFCpv: Code<'static> = c("122189", "Pulmonary Venous Content (FCpv)");
pub const PulseRate: Code<'static> = c("113791", "Pulse Rate");
pub const PulseSequenceName: Code<'static> = c("128230", "Pulse Sequence Name");
pub const PulseStrength: Code<'static> = c("122195", "Pulse Strength");
pub const PulseTrainDefinition: Code<'static> = c("109063", "Pulse train definition");
pub const PulseWidth: Code<'static> = c("113793", "Pulse Width");
pub const Pulsed: Code<'static> = c("113631", "Pulsed");
pub const QANode: Code<'static> = c("130366", "QA Node");
pub const QANodePair: Code<'static> = c("130365", "QA Node Pair");
pub const QAProcessNote: Code<'static> = c("130036", "QA Process Note");
pub const QWave: Code<'static> = c("109042", "Q wave");
pub const QuadrantLocation: Code<'static> = c("111048", "Quadrant location");
pub const QuadrantOctantSectors: Code<'static> = c("131306", "Quadrant-octant sectors");
pub const QuadrantSectors: Code<'static> = c("131302", "Quadrant sectors");
pub const QuadraticCellKillFactor: Code<'static> = c("130089", "Quadratic Cell Kill Factor");
pub const QualitativeAnalysis: Code<'static> = c("122145", "Qualitative Analysis");
pub const QualitativeDifference: Code<'static> = c("111049", "Qualitative Difference");
pub const QualityAssessment: Code<'static> = c("111050", "Quality Assessment");
pub const QualityControl: Code<'static> = c("110002", "Quality Control");
pub const QualityControlIntent: Code<'static> = c("113680", "Quality Control Intent");
pub const QualityControlMaterialImage: Code<'static> = c("130750", "Quality control material image");
pub const QualityControlStandard: Code<'static> = c("111051", "Quality Control Standard");
pub const QualityFinding: Code<'static> = c("111052", "Quality Finding");
pub const QualityIssue: Code<'static> = c("113010", "Quality Issue");
pub const QuantitativeAnalysis: Code<'static> = c("122144", "Quantitative Analysis");
pub const QuantitativeArteriographyReport: Code<'static> = c("122291", "Quantitative Arteriography Report");
pub const QuantitativeVentriculographyReport: Code<'static> = c("122292", "Quantitative Ventriculography Report");
pub const QuantityAdministered: Code<'static> = c("121382", "Quantity administered");
pub const QuantityOfMaterial: Code<'static> = c("121146", "Quantity of Material");
pub const Query: Code<'static> = c("110112", "Query");
pub const QuotationMode: Code<'static> = c("121001", "Quotation Mode");
pub const QuotedSource: Code<'static> = c("121002", "Quoted Source");
pub const R1: Code<'static> = c("126393", "R1");
pub const R150789Zr: Code<'static> = c("126518", "R1507 ^89^Zr");
pub const R2Coefficient: Code<'static> = c("126220", "R2-Coefficient");
pub const R2_126394: Code<'static> = c("126394", "R2");
pub const R2_126395: Code<'static> = c("126395", "R2*");
pub const RADAR2017: Code<'static> = c("113525", "RADAR 2017");
pub const RCARegionInLAOProjection: Code<'static> = c("122471", "RCA Region in LAO Projection");
pub const RCARegionInRAOProjection: Code<'static> = c("122465", "RCA Region in RAO Projection");
pub const RCoefficient: Code<'static> = c("113057", "R-Coefficient");
pub const RDSRFrameOfReferenceDescription: Code<'static> = c("130507", "RDSR Frame of Reference Description");
pub const RDSRFrameOfReferenceOrigin: Code<'static> = c("130506", "RDSR Frame of Reference Origin");
pub const RECIST: Code<'static> = c("112022", "RECIST");
pub const RECIST10: Code<'static> = c("126080", "RECIST 1.0");
pub const RECIST11: Code<'static> = c("126081", "RECIST 1.1");
pub const RGBBComponent: Code<'static> = c("110836", "RGB B Component");
pub const RGBGComponent: Code<'static> = c("110835", "RGB G Component");
pub const RGBRComponent: Code<'static> = c("110834", "RGB R Component");
pub const RNFLAverageThickness: Code<'static> = c("131264", "RNFL average thickness");
pub const RNFLClockfaceMethod: Code<'static> = c("131308", "RNFL Clockface Method");
pub const RNFLClockfacePosition10Thickness: Code<'static> = c("131285", "RNFL clockface position 10 thickness");
pub const RNFLClockfacePosition11Thickness: Code<'static> = c("131286", "RNFL clockface position 11 thickness");
pub const RNFLClockfacePosition12Thickness: Code<'static> = c("131287", "RNFL clockface position 12 thickness");
pub const RNFLClockfacePosition1Thickness: Code<'static> = c("131276", "RNFL clockface position 1 thickness");
pub const RNFLClockfacePosition2Thickness: Code<'static> = c("131277", "RNFL clockface position 2 thickness");
pub const RNFLClockfacePosition3Thickness: Code<'static> = c("131278", "RNFL clockface position 3 thickness");
pub const RNFLClockfacePosition4Thickness: Code<'static> = c("131279", "RNFL clockface position 4 thickness");
pub const RNFLClockfacePosition5Thickness: Code<'static> = c("131280", "RNFL clockface position 5 thickness");
pub const RNFLClockfacePosition6Thickness: Code<'static> = c("131281", "RNFL clockface position 6 thickness");
pub const RNFLClockfacePosition7Thickness: Code<'static> = c("131282", "RNFL clockface position 7 thickness");
pub const RNFLClockfacePosition8Thickness: Code<'static> = c("131283", "RNFL clockface position 8 thickness");
pub const RNFLClockfacePosition9Thickness: Code<'static> = c("131284", "RNFL clockface position 9 thickness");
pub const RNFLInferiorSectorThickness: Code<'static> = c("131265", "RNFL inferior sector thickness");
pub const RNFLNasalInferiorSectorThickness: Code<'static> = c("131270", "RNFL nasal-inferior sector thickness");
pub const RNFLNasalSectorThickness: Code<'static> = c("131268", "RNFL nasal sector thickness");
pub const RNFLNasalSuperiorSectorThickness: Code<'static> = c("131269", "RNFL nasal-superior sector thickness");
pub const RNFLSuperiorSectorThickness: Code<'static> = c("131266", "RNFL superior sector thickness");
pub const RNFLTemporalInferiorSectorThickness: Code<'static> = c("131271", "RNFL temporal-inferior sector thickness");
pub const RNFLTemporalSectorThickness: Code<'static> = c("131267", "RNFL temporal sector thickness");
pub const RNFLTemporalSuperiorSectorThickness: Code<'static> = c("131272", "RNFL temporal-superior sector thickness");
pub const RNFLVascularPlexusFlow: Code<'static> = c("128312", "RNFL vascular plexus flow");
pub const RNFLVascularPlexusMap: Code<'static> = c("128313", "RNFL vascular plexus map");
pub const RO532344189Zr: Code<'static> = c("126755", "RO5323441 ^89^Zr");
pub const RO54290889Zr: Code<'static> = c("126756", "RO542908 ^89^Zr");
pub const RO692496311C: Code<'static> = c("126719", "RO6924963 ^11^C");
pub const RO693164311C: Code<'static> = c("126720", "RO6931643 ^11^C");
pub const RO695894818F: Code<'static> = c("126757", "RO6958948 ^18^F");
pub const ROIDepth: Code<'static> = c("130613", "ROI Depth");
pub const RPLGD: Code<'static> = c("128709", "RPLGD");
pub const RRInterval: Code<'static> = c("122182", "R-R interval");
pub const RTBeamEnergy: Code<'static> = c("130034", "RT Beam Energy");
pub const RTBrachyTreatment: Code<'static> = c("121735", "RT Brachy Treatment");
pub const RTDose: Code<'static> = c("RTDOSE", "RT Dose");
pub const RTDoseCalculationStructure: Code<'static> = c("130042", "RT Dose Calculation Structure");
pub const RTGeometricInformation: Code<'static> = c("130043", "RT Geometric Information");
pub const RTImage: Code<'static> = c("RTIMAGE", "RT Image");
pub const RTImageGuidedPatientPositioningAndTreatmentDelivery: Code<'static> = c("121736", "RT Image-Guided Patient Positioning and Treatment Delivery");
pub const RTMachineQA: Code<'static> = c("121730", "RT Machine QA");
pub const RTPatientPositionAcquisitionCTKV: Code<'static> = c("121707", "RT Patient Position Acquisition, CT kV");
pub const RTPatientPositionAcquisitionCTMV: Code<'static> = c("121708", "RT Patient Position Acquisition, CT MV");
pub const RTPatientPositionAcquisitionConeBeamCTKV: Code<'static> = c("130785", "RT Patient Position Acquisition, Cone-Beam CT kV");
pub const RTPatientPositionAcquisitionConeBeamCTMV: Code<'static> = c("130787", "RT Patient Position Acquisition, Cone-Beam CT MV");
pub const RTPatientPositionAcquisitionConventionalCTKV: Code<'static> = c("130786", "RT Patient Position Acquisition, Conventional CT kV");
pub const RTPatientPositionAcquisitionConventionalCTMV: Code<'static> = c("130788", "RT Patient Position Acquisition, Conventional CT MV");
pub const RTPatientPositionAcquisitionDualPlaneKV: Code<'static> = c("121705", "RT Patient Position Acquisition, dual plane kV");
pub const RTPatientPositionAcquisitionDualPlaneKVPerMV: Code<'static> = c("121706", "RT Patient Position Acquisition, dual plane kV/MV");
pub const RTPatientPositionAcquisitionDualPlaneMV: Code<'static> = c("121703", "RT Patient Position Acquisition, dual plane MV");
pub const RTPatientPositionAcquisitionFilmCassetteKV: Code<'static> = c("130784", "RT Patient Position Acquisition, Film Cassette kV");
pub const RTPatientPositionAcquisitionFilmCassetteMV: Code<'static> = c("130783", "RT Patient Position Acquisition, Film Cassette MV");
pub const RTPatientPositionAcquisitionIntegratedDoseMV: Code<'static> = c("130782", "RT Patient Position Acquisition, Integrated Dose MV");
pub const RTPatientPositionAcquisitionMR: Code<'static> = c("121737", "RT Patient Position Acquisition, MR");
pub const RTPatientPositionAcquisitionOptical: Code<'static> = c("121709", "RT Patient Position Acquisition, Optical");
pub const RTPatientPositionAcquisitionSinglePlaneKV: Code<'static> = c("121704", "RT Patient Position Acquisition, single plane kV");
pub const RTPatientPositionAcquisitionSinglePlaneMV: Code<'static> = c("121702", "RT Patient Position Acquisition, single plane MV");
pub const RTPatientPositionAcquisitionSpatialFiducials: Code<'static> = c("121711", "RT Patient Position Acquisition, Spatial Fiducials");
pub const RTPatientPositionAcquisitionUltrasound: Code<'static> = c("121710", "RT Patient Position Acquisition, Ultrasound");
pub const RTPatientPositionAdjustment: Code<'static> = c("121722", "RT Patient Position Adjustment");
pub const RTPatientPositionInTreatmentSessionReview: Code<'static> = c("121723", "RT Patient Position In-treatment-session Review");
pub const RTPatientPositionRegistration2DOn3DReference: Code<'static> = c("121718", "RT Patient Position Registration, 2D on 3D reference");
pub const RTPatientPositionRegistration3DCTGeneral: Code<'static> = c("121714", "RT Patient Position Registration, 3D CT general");
pub const RTPatientPositionRegistration3DCTMarkerBased: Code<'static> = c("121715", "RT Patient Position Registration, 3D CT marker-based");
pub const RTPatientPositionRegistration3DCTVolumeBased: Code<'static> = c("121716", "RT Patient Position Registration, 3D CT volume-based");
pub const RTPatientPositionRegistration3DOn2DReference: Code<'static> = c("121717", "RT Patient Position Registration, 3D on 2D reference");
pub const RTPatientPositionRegistrationDualPlane: Code<'static> = c("121713", "RT Patient Position Registration, dual plane");
pub const RTPatientPositionRegistrationMR: Code<'static> = c("121738", "RT Patient Position Registration, MR");
pub const RTPatientPositionRegistrationOptical: Code<'static> = c("121719", "RT Patient Position Registration, Optical");
pub const RTPatientPositionRegistrationSinglePlane: Code<'static> = c("121712", "RT Patient Position Registration, single plane");
pub const RTPatientPositionRegistrationSpatialFiducials: Code<'static> = c("121721", "RT Patient Position Registration, Spatial Fiducials");
pub const RTPatientPositionRegistrationUltrasound: Code<'static> = c("121720", "RT Patient Position Registration, Ultrasound");
pub const RTPatientPositioningReferenceImage: Code<'static> = c("130790", "RT Patient Positioning Reference Image");
pub const RTPatientSetup: Code<'static> = c("121701", "RT Patient Setup");
pub const RTPlan: Code<'static> = c("RTPLAN", "RT Plan");
pub const RTPlanLabel: Code<'static> = c("121384", "RT Plan Label");
pub const RTPlanSummationInputUsed: Code<'static> = c("128212", "RT Plan Summation Input Used");
pub const RTPlanningResult: Code<'static> = c("128189", "RT Planning Result");
pub const RTPreTreatmentConsistencyCheck: Code<'static> = c("121374", "RT Pre-Treatment Consistency Check");
pub const RTPreTreatmentDoseCheck: Code<'static> = c("121373", "RT Pre-Treatment Dose Check");
pub const RTPrescriptionInputImages: Code<'static> = c("130136", "RT Prescription Input Images");
pub const RTPrescriptionInputUsed: Code<'static> = c("128210", "RT Prescription Input Used");
pub const RTPrescriptionResult: Code<'static> = c("128185", "RT Prescription Result");
pub const RTRadiationPreviouslyDelivered: Code<'static> = c("130663", "RT Radiation previously delivered");
pub const RTRegistrationMark: Code<'static> = c("130746", "RT Registration Mark");
pub const RTSegmentMaterial: Code<'static> = c("130737", "RT Segment Material");
pub const RTStructureSet: Code<'static> = c("RTSTRUCT", "RT Structure Set");
pub const RTTarget: Code<'static> = c("130041", "RT Target");
pub const RTTreatmentCourseSummary: Code<'static> = c("128193", "RT Treatment Course Summary");
pub const RTTreatmentPlanForThePositionBeingVerified: Code<'static> = c("121310", "RT treatment plan for the position being verified");
pub const RTTreatmentPlanningInputImages: Code<'static> = c("130137", "RT Treatment Planning Input Images");
pub const RTTreatmentPlanningInputUsed: Code<'static> = c("128211", "RT Treatment Planning Input Used");
pub const RTTreatmentQAByRTIonPlanDifferenceCheck: Code<'static> = c("121734", "RT Treatment QA by RT Ion Plan Difference Check");
pub const RTTreatmentQAByRTIonPlanDoseCheck: Code<'static> = c("121733", "RT Treatment QA by RT Ion Plan Dose Check");
pub const RTTreatmentQAByRTPlanDifferenceCheck: Code<'static> = c("121732", "RT Treatment QA by RT Plan Difference Check");
pub const RTTreatmentQAByRTPlanDoseCheck: Code<'static> = c("121731", "RT Treatment QA by RT Plan Dose Check");
pub const RTTreatmentQAResult: Code<'static> = c("128194", "RT Treatment QA Result");
pub const RTTreatmentQAWithExternalVerification: Code<'static> = c("121729", "RT Treatment QA with External Verification");
pub const RTTreatmentQAWithInternalVerification: Code<'static> = c("121728", "RT Treatment QA with Internal Verification");
pub const RTTreatmentRecord: Code<'static> = c("RTRECORD", "RT Treatment Record");
pub const RTTreatmentSessionResult: Code<'static> = c("128192", "RT Treatment Session Result");
pub const RTTreatmentSimulationResult: Code<'static> = c("128188", "RT Treatment Simulation Result");
pub const RTTreatmentSimulationWithExternalVerification: Code<'static> = c("121725", "RT Treatment Simulation with External Verification");
pub const RTTreatmentSimulationWithInternalVerification: Code<'static> = c("121724", "RT Treatment Simulation with Internal Verification");
pub const RTTreatmentWithAdHocPlanning: Code<'static> = c("130444", "RT Treatment with Ad Hoc Planning");
pub const RTTreatmentWithExternalVerification: Code<'static> = c("121727", "RT Treatment with External Verification");
pub const RTTreatmentWithInternalVerification: Code<'static> = c("121726", "RT Treatment with Internal Verification");
pub const RTVAudioAndStereoVideoRendition: Code<'static> = c("130373", "RTV Audio and Stereo Video Rendition");
pub const RTVAudioAndVideoRendition: Code<'static> = c("130371", "RTV Audio and Video Rendition");
pub const RTVRendition: Code<'static> = c("130370", "RTV Rendition");
pub const RTVStereoVideoRendition: Code<'static> = c("130372", "RTV Stereo Video Rendition");
pub const RTWorkflowInputUsed: Code<'static> = c("128209", "RT Workflow Input Used");
pub const RWave: Code<'static> = c("109043", "R wave");
pub const RadialBScanPattern: Code<'static> = c("128282", "Radial B-scan pattern");
pub const RadialBasedWallMotionAnalysis: Code<'static> = c("122493", "Radial Based Wall Motion Analysis");
pub const RadialDiffusivity: Code<'static> = c("113203", "Radial Diffusivity");
pub const RadialKurtosis: Code<'static> = c("113207", "Radial Kurtosis");
pub const RadialLagrangianStrain: Code<'static> = c("130675", "Radial Lagrangian Strain");
pub const RadialLagrangianStrainRate: Code<'static> = c("130676", "Radial Lagrangian Strain Rate");
pub const RadialPeripapillaryStructuralReflectanceMap: Code<'static> = c("128264", "Radial peripapillary structural reflectance map");
pub const RadialPeripapillaryVasculatureFlow: Code<'static> = c("128263", "Radial peripapillary vasculature flow");
#[deprecated = "retired in PS3.16"]
pub const RadialScar: Code<'static> = c("111293", "Radial scar");
pub const RadialShortening: Code<'static> = c("122496", "Radial Shortening");
pub const RadialUltrasoundTransducerGeometry: Code<'static> = c("125255", "Radial ultrasound transducer geometry");
pub const RadiationApplied: Code<'static> = c("122308", "Radiation applied");
pub const RadiationCharacteristicsNote: Code<'static> = c("130022", "Radiation Characteristics Note");
pub const RadiationDetectedDuringBeamHoldInterlock: Code<'static> = c("130828", "Radiation Detected During Beam Hold Interlock");
pub const RadiationDose: Code<'static> = c("131400", "Radiation Dose");
pub const RadiationDoseCompositeParameters: Code<'static> = c("128436", "Radiation Dose Composite Parameters");
pub const RadiationDoseEstimate: Code<'static> = c("128402", "Radiation Dose Estimate");
pub const RadiationDoseEstimateMethod: Code<'static> = c("128476", "Radiation Dose Estimate Method");
pub const RadiationDoseEstimateMethodReference: Code<'static> = c("128482", "Radiation Dose Estimate Method Reference");
pub const RadiationDoseEstimateMethodType: Code<'static> = c("128477", "Radiation Dose Estimate Method Type");
pub const RadiationDoseEstimateMethodology: Code<'static> = c("128415", "Radiation Dose Estimate Methodology");
pub const RadiationDoseEstimateName: Code<'static> = c("128403", "Radiation Dose Estimate Name");
#[deprecated = "retired in PS3.16"]
pub const RadiationDoseEstimateParameterType: Code<'static> = c("128464", "Radiation Dose Estimate Parameter Type");
pub const RadiationDoseEstimateParameters: Code<'static> = c("128434", "Radiation Dose Estimate Parameters");
pub const RadiationDoseEstimateRepresentation: Code<'static> = c("128412", "Radiation Dose Estimate Representation");
pub const RadiationDoseNormalizationPoint: Code<'static> = c("130063", "Radiation Dose Normalization Point");
pub const RadiationDoseReferencePoint: Code<'static> = c("130064", "Radiation Dose Reference Point");
pub const RadiationDoseRepresentationData: Code<'static> = c("128414", "Radiation Dose Representation Data");
pub const RadiationDoseShapingVolume: Code<'static> = c("130061", "Radiation Dose Shaping Volume");
pub const RadiationExposure: Code<'static> = c("113921", "Radiation Exposure");
#[deprecated = "retired in PS3.16"]
pub const RadiationExposureAndProtectionInformation: Code<'static> = c("113923", "Radiation Exposure and Protection Information");
pub const RadiationFieldArea: Code<'static> = c("130516", "Radiation Field Area");
pub const RadiationFieldOutline: Code<'static> = c("130517", "Radiation Field Outline");
pub const RadiationInteractionVolume: Code<'static> = c("130066", "Radiation Interaction Volume");
pub const RadiationOutput: Code<'static> = c("130514", "Radiation Output");
#[deprecated = "retired in PS3.16"]
pub const RadiationPhysicist: Code<'static> = c("121105", "Radiation Physicist");
pub const RadiationRemoved: Code<'static> = c("122309", "Radiation removed");
pub const RadiationSourceCharacteristics: Code<'static> = c("130508", "Radiation Source Characteristics");
pub const RadiationTechnique: Code<'static> = c("130511", "Radiation Technique");
pub const RadiationTherapyDevice: Code<'static> = c("RT", "Radiation Therapy Device");
pub const RadiationTherapyParticle: Code<'static> = c("130033", "Radiation Therapy Particle");
pub const RadiationTransportBasedMethods: Code<'static> = c("130126", "Radiation transport-based methods");
pub const RadiationTransportModelType: Code<'static> = c("128420", "Radiation Transport Model Type");
pub const RadioFrequencyAblationPower: Code<'static> = c("109010", "Radio frequency ablation, power");
pub const RadioactiveMarker: Code<'static> = c("130646", "Radioactive Marker");
#[deprecated = "retired in PS3.16"]
pub const RadioactiveSubstanceAdministered: Code<'static> = c("113922", "Radioactive Substance Administered");
pub const Radiofluoroscopy: Code<'static> = c("RF", "Radiofluoroscopy");
pub const RadiofrequencyTransponder: Code<'static> = c("130644", "Radiofrequency Transponder");
#[deprecated = "retired in PS3.16"]
pub const Radiographer: Code<'static> = c("121084", "Radiographer");
pub const RadiographicAnatomy: Code<'static> = c("112005", "Radiographic anatomy");
pub const RadiographicImaging: Code<'static> = c("RG", "Radiographic imaging");
pub const RadioisotopeGenerator: Code<'static> = c("113543", "Radioisotope Generator");
pub const RadionuclideResidualSyringeCounts: Code<'static> = c("123010", "Radionuclide Residual Syringe Counts");
pub const RadionuclideSyringeCounts: Code<'static> = c("123009", "Radionuclide Syringe Counts");
pub const RadionuclideTotalDose: Code<'static> = c("123006", "Radionuclide Total Dose");
pub const RadionuclideVialIdentifier: Code<'static> = c("113514", "Radionuclide Vial Identifier");
#[deprecated = "retired in PS3.16"]
pub const Radiopharmaceutical: Code<'static> = c("123001", "Radiopharmaceutical");
pub const RadiopharmaceuticalAdministration: Code<'static> = c("113502", "Radiopharmaceutical Administration");
pub const RadiopharmaceuticalAdministrationEventUID: Code<'static> = c("113503", "Radiopharmaceutical Administration Event UID");
pub const RadiopharmaceuticalDispenseUnitIdentifier: Code<'static> = c("113511", "Radiopharmaceutical Dispense Unit Identifier");
pub const RadiopharmaceuticalLotIdentifier: Code<'static> = c("113512", "Radiopharmaceutical Lot Identifier");
pub const RadiopharmaceuticalRadiationDoseReport: Code<'static> = c("113500", "Radiopharmaceutical Radiation Dose Report");
pub const RadiopharmaceuticalSpecificActivity: Code<'static> = c("123007", "Radiopharmaceutical Specific Activity");
pub const RadiopharmaceuticalStartDateTime: Code<'static> = c("123003", "Radiopharmaceutical Start DateTime");
pub const RadiopharmaceuticalStopDateTime: Code<'static> = c("123004", "Radiopharmaceutical Stop DateTime");
pub const RadiopharmaceuticalVolume: Code<'static> = c("123005", "Radiopharmaceutical Volume");
pub const RadiotherapyApplicator: Code<'static> = c("130125", "Radiotherapy Applicator");
pub const RadiotherapyDoseRegion: Code<'static> = c("130748", "Radiotherapy Dose Region");
pub const RadiotherapyFiducial: Code<'static> = c("130666", "Radiotherapy Fiducial");
pub const RadiotherapyTreatmentDevice: Code<'static> = c("130361", "Radiotherapy Treatment Device");
pub const RaisedSkinLesion: Code<'static> = c("130486", "Raised skin lesion");
pub const Ranibizumab89Zr: Code<'static> = c("126742", "Ranibizumab ^89^Zr");
pub const RasterBScanPattern: Code<'static> = c("128280", "Raster B-scan pattern");
pub const RateOfAdministration: Code<'static> = c("122094", "Rate of administration");
pub const RateOfExposure: Code<'static> = c("111579", "Rate of exposure");
pub const RatingOfPerceivedExertion: Code<'static> = c("122706", "Rating of Perceived Exertion");
pub const RatioOfAchievedToPredictedFunctionalCapacity: Code<'static> = c("122771", "Ratio of achieved to predicted functional capacity");
pub const RatioOfAchievedToPredictedMaximalOxygenConsumption: Code<'static> = c("122770", "Ratio of achieved to predicted maximal oxygen consumption");
pub const ReaderSpecialty: Code<'static> = c("128003", "Reader Specialty");
pub const ReagentVialIdentifier: Code<'static> = c("113513", "Reagent Vial Identifier");
pub const RealTimeAcquisition: Code<'static> = c("109080", "Real time acquisition");
pub const RealWorldValueMap: Code<'static> = c("RWV", "Real World Value Map");
pub const RealWorldValueMapUsedForMeasurement: Code<'static> = c("126100", "Real World Value Map used for measurement");
pub const ReasonForBiosafetyControls: Code<'static> = c("127011", "Reason for biosafety controls");
#[deprecated = "retired in PS3.16"]
pub const ReasonForProcedure: Code<'static> = c("111401", "Reason for Procedure");
pub const ReasonForProceeding: Code<'static> = c("113907", "Reason for Proceeding");
pub const ReasonForRejectingAcquisition: Code<'static> = c("130504", "Reason for Rejecting Acquisition");
pub const ReasonForRepeatingAcquisition: Code<'static> = c("128552", "Reason for Repeating Acquisition");
pub const ReasonForStudy: Code<'static> = c("122139", "Reason for Study");
pub const RecallForImagingFindings: Code<'static> = c("111591", "Recall for imaging findings");
pub const RecallForPatientSymptomsPerClinicalFindings: Code<'static> = c("111592", "Recall for patient symptoms/ clinical findings");
pub const RecallForTechnicalReasons: Code<'static> = c("111590", "Recall for technical reasons");
pub const RecentPhysicalActivity: Code<'static> = c("113552", "Recent Physical Activity");
pub const ReciprocatingGrid: Code<'static> = c("111643", "Reciprocating grid");
pub const Recommendation: Code<'static> = c("121075", "Recommendation");
#[deprecated = "retired in PS3.16"]
pub const Recommendations: Code<'static> = c("121074", "Recommendations");
pub const RecommendedCADOperatingPoint: Code<'static> = c("111092", "Recommended CAD Operating Point");
pub const RecommendedFollowUp: Code<'static> = c("111053", "Recommended Follow-up");
pub const RecommendedFollowUpDate: Code<'static> = c("111054", "Recommended Follow-up Date");
pub const RecommendedFollowUpInterval: Code<'static> = c("111055", "Recommended Follow-up Interval");
pub const Reconstructed: Code<'static> = c("122404", "Reconstructed");
pub const ReconstructionAlgorithm: Code<'static> = c("113961", "Reconstruction Algorithm");
pub const Recording: Code<'static> = c("121097", "Recording");
#[deprecated = "retired in PS3.16"]
pub const RecoveryState: Code<'static> = c("109096", "Recovery state");
pub const RectalBalloon: Code<'static> = c("130120", "Rectal Balloon");
pub const RecumbentPatientPositionWithRespectToGravity: Code<'static> = c("112228", "Recumbent Patient Position with respect to gravity");
pub const RecurrentMalignancy: Code<'static> = c("111338", "Recurrent malignancy");
#[deprecated = "retired in PS3.16"]
pub const RedFilter: Code<'static> = c("111602", "Red filter");
pub const RedTranslucentIgloo: Code<'static> = c("127221", "Red translucent igloo");
pub const RedistributionState: Code<'static> = c("109093", "Redistribution State");
pub const ReferenceAcquisitionPoint: Code<'static> = c("130072", "Reference Acquisition Point");
pub const ReferenceAuthority: Code<'static> = c("121406", "Reference Authority");
pub const ReferenceBasis: Code<'static> = c("128772", "Reference Basis");
pub const ReferenceEnergy: Code<'static> = c("130087", "Reference Energy");
pub const ReferenceGeometry: Code<'static> = c("128773", "Reference Geometry");
pub const ReferenceMAs: Code<'static> = c("131552", "Reference mAs");
pub const ReferenceMeasurementGroup: Code<'static> = c("130755", "Reference Measurement Group");
pub const ReferenceMethod: Code<'static> = c("122430", "Reference Method");
pub const ReferencePoint: Code<'static> = c("122611", "Reference Point");
pub const ReferencePointDefinition: Code<'static> = c("113780", "Reference Point Definition");
pub const ReferencePointDosimetry: Code<'static> = c("130502", "Reference Point Dosimetry");
pub const ReferencePointPosition: Code<'static> = c("130526", "Reference Point Position");
pub const ReferencePoints: Code<'static> = c("122438", "Reference Points");
pub const ReferenceToUncertaintyDeterminationMethod: Code<'static> = c("128511", "Reference to Uncertainty Determination Method");
pub const ReferencedBeamNumber: Code<'static> = c("121389", "Referenced Beam Number");
pub const ReferencedFrames: Code<'static> = c("121190", "Referenced Frames");
pub const ReferencedImagingAgentAdministrationPhaseIdentifier: Code<'static> = c("130217", "Referenced Imaging Agent Administration Phase Identifier");
pub const ReferencedImagingAgentAdministrationPhaseUID: Code<'static> = c("130262", "Referenced Imaging Agent Administration Phase UID");
pub const ReferencedImagingAgentAdministrationStepUID: Code<'static> = c("130216", "Referenced Imaging Agent Administration Step UID");
pub const ReferencedImagingAgentIdentifier: Code<'static> = c("130255", "Referenced Imaging Agent Identifier");
pub const ReferencedPatientAlignmentReference: Code<'static> = c("130662", "Referenced Patient Alignment Reference");
pub const ReferencedRegionOfInterestIdentifier: Code<'static> = c("130489", "Referenced Region of Interest Identifier");
pub const ReferencedSegment: Code<'static> = c("121191", "Referenced Segment");
pub const ReferencedSegmentationFrame: Code<'static> = c("121214", "Referenced Segmentation Frame");
#[deprecated = "retired in PS3.16"]
pub const Referring: Code<'static> = c("121095", "Referring");
pub const ReflectionBasedCornealTopographer: Code<'static> = c("111946", "Reflection-based corneal topographer");
pub const ReflectionIllumination: Code<'static> = c("111742", "Reflection illumination");
pub const RefractiveMeasurementsSOPInstance: Code<'static> = c("111783", "Refractive Measurements SOP Instance");
pub const RegionInSpace: Code<'static> = c("130488", "Region in Space");
pub const RegionalAbnormalWallMotion: Code<'static> = c("122417", "Regional Abnormal Wall Motion");
#[deprecated = "retired in PS3.16"]
pub const RegionalCerebralBloodFlow: Code<'static> = c("113055", "Regional Cerebral Blood Flow");
#[deprecated = "retired in PS3.16"]
pub const RegionalCerebralBloodVolume: Code<'static> = c("113056", "Regional Cerebral Blood Volume");
pub const RegionalContributionToEjectionFraction: Code<'static> = c("122495", "Regional Contribution to Ejection Fraction");
#[deprecated = "retired in PS3.16"]
pub const Registrar: Code<'static> = c("121087", "Registrar");
pub const Registration: Code<'static> = c("REG", "Registration");
pub const RegistrationCreatedDuringTreatment: Code<'static> = c("129211", "Registration created during Treatment");
pub const RegistrationInput: Code<'static> = c("125041", "Registration Input");
pub const RegistrationMethod: Code<'static> = c("128446", "Registration Method");
pub const RegistrationResult: Code<'static> = c("128183", "Registration Result");
pub const RegistrationUsedInPlanning: Code<'static> = c("129210", "Registration used in Planning");
pub const RegressionOffsetED: Code<'static> = c("122432", "Regression Offset ED");
pub const RegressionOffsetES: Code<'static> = c("122434", "Regression Offset ES");
pub const RegressionSlopeED: Code<'static> = c("122431", "Regression Slope ED");
pub const RegressionSlopeES: Code<'static> = c("122433", "Regression Slope ES");
pub const RegressionVolumeExponent: Code<'static> = c("122435", "Regression Volume Exponent");
pub const RegurgitationJetArea: Code<'static> = c("125332", "Regurgitation Jet Area");
pub const RegurgitationJetWidth: Code<'static> = c("125333", "Regurgitation Jet Width");
pub const ReinjectionState: Code<'static> = c("109092", "Reinjection State");
pub const RejectedForPatientSafetyReasons: Code<'static> = c("113037", "Rejected for Patient Safety Reasons");
pub const RejectedForQualityReasons: Code<'static> = c("113001", "Rejected for Quality Reasons");
pub const RejectionWithdrawn: Code<'static> = c("131360", "Rejection Withdrawn");
pub const RelatedImplantationReports: Code<'static> = c("112365", "Related Implantation Reports");
pub const RelatedOphthalmicThicknessMap: Code<'static> = c("111933", "Related ophthalmic thickness map");
pub const RelatedPatientDataNotUsedDuringPlanning: Code<'static> = c("112364", "Related Patient Data Not Used During Planning");
pub const RelatedToNonMacularSensitivity: Code<'static> = c("111842", "Related to non macular sensitivity");
pub const RelativeAmountOfExposure: Code<'static> = c("111582", "Relative amount of exposure");
pub const RelativeAmountOfUse: Code<'static> = c("111583", "Relative amount of use");
pub const RelativeAnisotropy: Code<'static> = c("110809", "Relative Anisotropy");
pub const RelativeDoseAmount: Code<'static> = c("111581", "Relative dose amount");
pub const RelativeDoseFrequency: Code<'static> = c("111584", "Relative dose frequency");
pub const RelativeElectronDensity: Code<'static> = c("130083", "Relative Electron Density");
pub const RelativeFrequencyOfExposure: Code<'static> = c("111585", "Relative frequency of exposure");
pub const RelativeFrequencyOfUse: Code<'static> = c("111586", "Relative frequency of use");
pub const RelativeLinearStoppingPower: Code<'static> = c("130086", "Relative Linear Stopping Power");
pub const RelativeMassDensity: Code<'static> = c("130082", "Relative Mass Density");
pub const RelativePosition: Code<'static> = c("122337", "Relative position");
pub const RelativeRegionalBloodFlow: Code<'static> = c("126397", "Relative Regional Blood Flow");
pub const RelativeRegionalBloodVolume: Code<'static> = c("126398", "Relative Regional Blood Volume");
pub const RelativeStenosisSeverity: Code<'static> = c("122391", "Relative Stenosis Severity");
#[deprecated = "retired in PS3.16"]
pub const RelevantIndicatedProblems: Code<'static> = c("111514", "Relevant Indicated Problems");
pub const RelevantPatientInformation: Code<'static> = c("111517", "Relevant Patient Information");
pub const RelevantPatientInformationForBreastImaging: Code<'static> = c("111511", "Relevant Patient Information for Breast Imaging");
pub const RelevantPreviousProcedures: Code<'static> = c("111513", "Relevant Previous Procedures");
pub const RelevantRiskFactors: Code<'static> = c("111515", "Relevant Risk Factors");
pub const RemainingConsumableDisposed: Code<'static> = c("122078", "Remaining consumable disposed");
pub const RemodelingIndex: Code<'static> = c("122345", "Remodeling Index");
pub const RemoteServiceOperationStarted: Code<'static> = c("110139", "Remote Service Operation Started");
pub const RemoteServiceOperationStopped: Code<'static> = c("110140", "Remote Service Operation Stopped");
pub const RenderingIntent: Code<'static> = c("111056", "Rendering Intent");
pub const ReplacedByAlternateRepresentation: Code<'static> = c("130754", "Replaced by Alternate Representation");
pub const ReplacedReport: Code<'static> = c("121360", "Replaced report");
pub const ReportRequested: Code<'static> = c("128004", "Report Requested");
pub const ReportVerification: Code<'static> = c("110007", "Report Verification");
pub const ReportedValueType: Code<'static> = c("130510", "Reported Value Type");
pub const ReportingSystem: Code<'static> = c("130551", "Reporting system");
pub const RepositionedROIOrGrid: Code<'static> = c("131247", "Repositioned ROI or grid");
pub const RepresentativeOfEthicsCommittee: Code<'static> = c("128677", "Representative of Ethics Committee");
pub const RepresentativeOfProtocolCommittee: Code<'static> = c("128676", "Representative of Protocol Committee");
pub const RepresentativePoint: Code<'static> = c("128140", "Representative point");
#[deprecated = "retired in PS3.16"]
pub const Request: Code<'static> = c("121062", "Request");
pub const Requesting: Code<'static> = c("121096", "Requesting");
#[deprecated = "retired in PS3.16"]
pub const Resident: Code<'static> = c("121086", "Resident");
pub const ResidualVolumeOfImagingAgentInContainer: Code<'static> = c("130206", "Residual Volume of Imaging Agent in Container");
pub const ResourceInadequate: Code<'static> = c("110527", "Resource inadequate");
pub const ResourcePreEmpted: Code<'static> = c("110526", "Resource pre-empted");
pub const RespirationWaveform: Code<'static> = c("109117", "Respiration Waveform");
pub const Respiratory: Code<'static> = c("RESP", "Respiratory");
pub const RespiratoryEffort: Code<'static> = c("130433", "Respiratory Effort");
pub const RespiratoryPressure: Code<'static> = c("130426", "Respiratory Pressure");
pub const ResponseEvaluation: Code<'static> = c("112020", "Response Evaluation");
pub const ResponseEvaluationMethod: Code<'static> = c("112021", "Response Evaluation Method");
pub const RestTechnetiumPerStressTechnetium1DayProcedure: Code<'static> = c("122782", "Rest technetium/stress technetium 1 day procedure");
pub const RestTechnetiumPerStressTechnetium2DayProcedure: Code<'static> = c("122783", "Rest technetium/stress technetium 2 day procedure");
pub const RestThalliumPerStressTechnetiumProcedure: Code<'static> = c("122781", "Rest thallium/stress technetium procedure");
pub const RestenoticLesion: Code<'static> = c("122393", "Restenotic Lesion");
pub const ResultsCommunicated: Code<'static> = c("121291", "Results communicated");
pub const ResultsQualityController: Code<'static> = c("129004", "Results Quality Controller");
pub const ResumeProcedureAction: Code<'static> = c("121133", "Resume Procedure Action");
pub const RetainDeviceIdentityOption: Code<'static> = c("113109", "Retain Device Identity Option");
pub const RetainInstitutionIdentityOption: Code<'static> = c("113112", "Retain Institution Identity Option");
pub const RetainLongitudinalTemporalInformationFullDatesOption: Code<'static> = c("113106", "Retain Longitudinal Temporal Information Full Dates Option");
pub const RetainLongitudinalTemporalInformationModifiedDatesOption: Code<'static> = c("113107", "Retain Longitudinal Temporal Information Modified Dates Option");
pub const RetainPatientCharacteristicsOption: Code<'static> = c("113108", "Retain Patient Characteristics Option");
pub const RetainSafePrivateOption: Code<'static> = c("113111", "Retain Safe Private Option");
pub const RetainUIDsOption: Code<'static> = c("113110", "Retain UIDs Option");
pub const Reticle: Code<'static> = c("130350", "Reticle");
pub const ReticularPattern: Code<'static> = c("112113", "Reticular pattern");
pub const ReticulonodularPattern: Code<'static> = c("112065", "Reticulonodular pattern");
pub const RetinaDepthEncodedStructuralReflectanceMap: Code<'static> = c("128258", "Retina depth encoded structural reflectance map");
pub const RetinaDepthEncodedVasculatureFlow: Code<'static> = c("128257", "Retina depth encoded vasculature flow");
pub const RetinaStructuralReflectanceMap: Code<'static> = c("128260", "Retina structural reflectance map");
pub const RetinaVasculatureFlow: Code<'static> = c("128259", "Retina vasculature flow");
pub const RetinalNerveFiberLayerSymmetry: Code<'static> = c("131273", "Retinal nerve fiber layer symmetry");
pub const RetinalNerveFiberLayerThickness: Code<'static> = c("111925", "Retinal nerve fiber layer thickness");
pub const RetinalROIHeight: Code<'static> = c("131275", "Retinal ROI height");
pub const RetinalROIWidth: Code<'static> = c("131274", "Retinal ROI width");
pub const RetinalTopography: Code<'static> = c("111924", "Retinal topography");
pub const RetroOrbitalRoute: Code<'static> = c("127070", "Retro-orbital route");
pub const RetroPectoralImplant: Code<'static> = c("111486", "Retro-pectoral implant");
pub const RetronasalTriangle: Code<'static> = c("131372", "Retronasal triangle");
pub const RetrospectiveGating: Code<'static> = c("109082", "Retrospective gating");
pub const Reused: Code<'static> = c("127178", "Reused");
pub const ReverseOsmosisPurifiedHClAcidifiedWater: Code<'static> = c("127291", "Reverse osmosis purified, HCl acidified water");
pub const ReverseOsmosisPurifiedWater: Code<'static> = c("127290", "Reverse osmosis purified water");
pub const ReviewOfAnOutsideStudy: Code<'static> = c("111418", "Review of an outside study");
pub const RibScaleneTubercle: Code<'static> = c("112096", "Rib Scalene Tubercle");
pub const RightArmRestPitchAngle: Code<'static> = c("130849", "Right Arm Rest Pitch Angle");
pub const RightArmRestPosition: Code<'static> = c("130848", "Right Arm Rest Position");
pub const RightArmRestRollAngle: Code<'static> = c("130850", "Right Arm Rest Roll Angle");
pub const RightContour: Code<'static> = c("122508", "Right Contour");
pub const RightCoronaryArteryOstiumHeight: Code<'static> = c("131169", "Right coronary artery ostium height");
pub const RightEyeRx: Code<'static> = c("111688", "Right Eye Rx");
pub const RightFirst: Code<'static> = c("126831", "right first");
pub const RightHemisphereMostAnterior: Code<'static> = c("125031", "Right Hemisphere Most Anterior");
pub const RightHemisphereMostInferior: Code<'static> = c("125034", "Right Hemisphere Most Inferior");
pub const RightHemisphereMostPosterior: Code<'static> = c("125032", "Right Hemisphere Most Posterior");
pub const RightHemisphereMostSuperior: Code<'static> = c("125033", "Right Hemisphere Most Superior");
pub const RightToLeft: Code<'static> = c("110866", "Right To Left");
pub const RightToLeftFlow: Code<'static> = c("122228", "Right to Left Flow");
pub const RightTrachealStripe: Code<'static> = c("112093", "Right tracheal stripe");
pub const RightVentricleAnteriorWall: Code<'static> = c("125319", "Right Ventricle Anterior Wall");
pub const RightVentricleCardiacOutput: Code<'static> = c("131053", "Right Ventricle Cardiac Output");
pub const RightVentricleDiastolicMajorAxis: Code<'static> = c("131170", "Right ventricle diastolic major axis");
pub const RightVentricleOutflowTractDistal: Code<'static> = c("125317", "Right Ventricle Outflow Tract, Distal");
pub const RightVentricleOutflowTractProximal: Code<'static> = c("125318", "Right Ventricle Outflow Tract, Proximal");
pub const RightVentricularDiastolicBasalMinorAxis: Code<'static> = c("131172", "Right ventricular diastolic basal minor axis");
pub const RightVentricularDiastolicMidSegmentMinorAxis: Code<'static> = c("131171", "Right ventricular diastolic mid segment minor axis");
pub const RightVentricularEjectionTime: Code<'static> = c("122213", "Right Ventricular ejection time");
pub const RightVentricularFillingTime: Code<'static> = c("122214", "Right Ventricular filling time");
pub const RightVentricularInletLength: Code<'static> = c("131017", "Right ventricular inlet length");
pub const RingUltrasoundTransducerGeometry: Code<'static> = c("125256", "Ring ultrasound transducer geometry");
pub const RiseTime: Code<'static> = c("130207", "Rise Time");
pub const RiskFactorModifier: Code<'static> = c("111530", "Risk Factor modifier");
pub const Rituximab89Zr: Code<'static> = c("126737", "Rituximab ^89^Zr");
pub const RobertsonAndReidTable: Code<'static> = c("122253", "Robertson & Reid table");
pub const RoleOfPersonReporting: Code<'static> = c("111534", "Role of person reporting");
pub const Roledumab89Zr: Code<'static> = c("126733", "Roledumab ^89^Zr");
pub const RoomBarometricPressure: Code<'static> = c("122225", "Room Barometric pressure");
pub const RoomIdentification: Code<'static> = c("121121", "Room identification");
pub const RoomLaserPatientSetupPoint: Code<'static> = c("130070", "Room Laser Patient Setup Point");
pub const RoomOrigin: Code<'static> = c("130536", "Room Origin");
pub const RoomOxygenConcentration: Code<'static> = c("122223", "Room oxygen concentration");
pub const RoomTemperature: Code<'static> = c("122224", "Room temperature");
pub const RootAngularSecondMomentOfGLCM: Code<'static> = c("126061", "Root Angular Second Moment of GLCM");
pub const RotationAngle: Code<'static> = c("130523", "Rotation Angle");
pub const RotationPlaneNormalPoint: Code<'static> = c("130522", "Rotation Plane Normal Point");
pub const RotationalAcquisition: Code<'static> = c("113613", "Rotational Acquisition");
#[deprecated = "retired in PS3.16"]
pub const RunEntropy: Code<'static> = c("128816", "Run Entropy");
#[deprecated = "retired in PS3.16"]
pub const RunLengthNonuniformity: Code<'static> = c("128811", "Run Length Nonuniformity");
#[deprecated = "retired in PS3.16"]
pub const RunLengthNonuniformityNormalized: Code<'static> = c("128812", "Run Length Nonuniformity Normalized");
#[deprecated = "retired in PS3.16"]
pub const RunLengthVariance: Code<'static> = c("128815", "Run Length Variance");
#[deprecated = "retired in PS3.16"]
pub const RunPercentage: Code<'static> = c("128813", "Run Percentage");
pub const RungeKutta: Code<'static> = c("113219", "Runge-Kutta");
pub const SAR341989Zr: Code<'static> = c("126741", "SAR3419 ^89^Zr");
pub const SMILE: Code<'static> = c("111681", "SMILE");
pub const SMPTEPattern: Code<'static> = c("109943", "SMPTE Pattern");
pub const SNITRectangularSectors: Code<'static> = c("131303", "SNIT rectangular sectors");
pub const SOPClassUID: Code<'static> = c("110181", "SOP Class UID");
pub const SPECTCTOfWholeBody: Code<'static> = c("127902", "SPECT CT of whole body");
pub const SPECTOfWholeBody: Code<'static> = c("127901", "SPECT of whole body");
pub const SPhase: Code<'static> = c("111477", "S Phase");
pub const SRInstanceUsed: Code<'static> = c("128416", "SR Instance Used");
pub const SRKII: Code<'static> = c("111766", "SRKII");
pub const SRKT: Code<'static> = c("111767", "SRK-T");
pub const SSDEConversionFactor: Code<'static> = c("128410", "SSDE Conversion Factor");
pub const SStent: Code<'static> = c("130733", "S - Stent");
pub const STChangeFromBaseline: Code<'static> = c("122099", "ST change from baseline");
pub const STDepressionDownsloping: Code<'static> = c("122759", "ST Depression - Downsloping");
pub const STDepressionHorizontal: Code<'static> = c("122757", "ST Depression - Horizontal");
pub const STDepressionUpsloping: Code<'static> = c("122758", "ST Depression - Upsloping");
pub const STElevationOnsetDateTime: Code<'static> = c("122173", "ST Elevation Onset DateTime");
pub const STPerHRIndex: Code<'static> = c("122773", "ST/HR Index");
pub const SUVBodySurfaceAreaCalculationMethod: Code<'static> = c("126412", "SUV body surface area calculation method");
pub const SUVBodyWeightCalculationMethod: Code<'static> = c("126410", "SUV body weight calculation method");
pub const SUVIdealBodyWeightCalculationMethod: Code<'static> = c("126413", "SUV ideal body weight calculation method");
pub const SUVLeanBodyMassCalculationJanmahasatianMethod: Code<'static> = c("126414", "SUV lean body mass calculation Janmahasatian method");
pub const SUVLeanBodyMassCalculationMethod: Code<'static> = c("126411", "SUV lean body mass calculation method");
pub const SUVLeanBodyMassCalculationMethodUsing128Multiplier: Code<'static> = c("126415", "SUV lean body mass calculation method using 128 multiplier");
pub const SUVbsa: Code<'static> = c("126403", "SUVbsa");
pub const SUVbw: Code<'static> = c("126401", "SUVbw");
pub const SUVibw: Code<'static> = c("126404", "SUVibw");
pub const SUVlbm: Code<'static> = c("126402", "SUVlbm");
pub const SUVlbmJames128: Code<'static> = c("126406", "SUVlbm(James128)");
pub const SUVlbmJanma: Code<'static> = c("126405", "SUVlbm(Janma)");
pub const SWave: Code<'static> = c("109044", "S wave");
pub const Saliency: Code<'static> = c("130404", "Saliency");
pub const SalineImplant: Code<'static> = c("111481", "Saline implant");
pub const SameAcquisitionAtDifferentFocalDepth: Code<'static> = c("121352", "Same acquisition at different focal depth");
pub const SameAcquisitionAtDifferentSpectralBand: Code<'static> = c("121353", "Same acquisition at different spectral band");
pub const SameAcquisitionAtHigherResolution: Code<'static> = c("121351", "Same acquisition at higher resolution");
pub const SameAcquisitionAtLowerResolution: Code<'static> = c("121350", "Same acquisition at lower resolution");
pub const SameAnatomy: Code<'static> = c("122401", "Same Anatomy");
pub const SameIndication: Code<'static> = c("122402", "Same Indication");
pub const SampleVolumeDepth: Code<'static> = c("125107", "Sample Volume Depth");
pub const SamplingDateTime: Code<'static> = c("111469", "Sampling DateTime");
pub const SamplingFrequency: Code<'static> = c("130882", "Sampling Frequency");
pub const SamplingMethod: Code<'static> = c("111704", "Sampling Method");
pub const ScanDelay: Code<'static> = c("130198", "Scan Delay");
pub const ScanSpotInterlock: Code<'static> = c("130824", "Scan Spot Interlock");
pub const ScanSpotMetersetInterlock: Code<'static> = c("130827", "Scan Spot Meterset Interlock");
pub const ScanSpotPositionInterlock: Code<'static> = c("130825", "Scan Spot Position Interlock");
pub const ScanSpotSizeInterlock: Code<'static> = c("130826", "Scan Spot Size Interlock");
pub const ScanningLength: Code<'static> = c("113825", "Scanning Length");
pub const ScapularInfraspinatusFossa: Code<'static> = c("112101", "Scapular Infraspinatus Fossa");
pub const ScapularSpine: Code<'static> = c("112099", "Scapular Spine");
pub const ScapularSupraspinatusFossa: Code<'static> = c("112100", "Scapular Supraspinatus Fossa");
#[deprecated = "retired in PS3.16"]
pub const ScarTissue: Code<'static> = c("111168", "Scar tissue");
pub const ScarredMyocardium: Code<'static> = c("122113", "Scarred Myocardium");
pub const ScheimpflugCamera: Code<'static> = c("111626", "Scheimpflug Camera");
pub const Schneider2005: Code<'static> = c("131531", "Schneider 2005");
#[deprecated = "retired in PS3.16"]
pub const SclerosingAdenosis: Code<'static> = c("111294", "Sclerosing adenosis");
pub const ScopeOfAccumulation: Code<'static> = c("113705", "Scope of Accumulation");
pub const ScopeOfFeature: Code<'static> = c("111057", "Scope of Feature");
pub const ScotopicLight: Code<'static> = c("111627", "Scotopic light");
pub const Scratch: Code<'static> = c("131515", "Scratch");
#[deprecated = "retired in PS3.16"]
pub const ScrubNurse: Code<'static> = c("121090", "Scrub nurse");
pub const SeatPan: Code<'static> = c("130855", "Seat Pan");
pub const SeatPanHeight: Code<'static> = c("130840", "Seat Pan Height");
pub const SeatPanPitchAngle: Code<'static> = c("130841", "Seat Pan Pitch Angle");
#[deprecated = "retired in PS3.16"]
pub const SecondMeasureOfInformationCorrelationOfGLCM: Code<'static> = c("128799", "Second Measure of Information Correlation of GLCM");
pub const SecondaryFluenceMonitoringSystemInterlock: Code<'static> = c("130476", "Secondary Fluence Monitoring System Interlock");
pub const SecondaryFollicle: Code<'static> = c("130911", "Secondary Follicle");
pub const SecondaryPulmonaryLobule: Code<'static> = c("112054", "Secondary pulmonary lobule");
pub const SectorUltrasoundTransducerGeometry: Code<'static> = c("125254", "Sector ultrasound transducer geometry");
pub const SecureDigitalCard: Code<'static> = c("110036", "Secure Digital Card");
pub const SecurityAlert: Code<'static> = c("110113", "Security Alert");
pub const SecurityConfiguration: Code<'static> = c("110129", "Security Configuration");
pub const SecurityRolesChanged: Code<'static> = c("110136", "Security Roles Changed");
pub const SeedPoint: Code<'static> = c("128139", "Seed point");
pub const SegmentationMethod: Code<'static> = c("122554", "Segmentation Method");
pub const SegmentationResult: Code<'static> = c("128182", "Segmentation Result");
pub const Segmentation_113076: Code<'static> = c("113076", "Segmentation");
pub const Segmentation_SEG: Code<'static> = c("SEG", "Segmentation");
pub const SegmentedConstrained: Code<'static> = c("113269", "Segmented-Constrained");
pub const SegmentedUnconstrained: Code<'static> = c("113268", "Segmented-Unconstrained");
pub const SelectedImplantComponent: Code<'static> = c("112346", "Selected Implant Component");
pub const SelectedRegion: Code<'static> = c("111099", "Selected region");
pub const SelectedRegionDescription: Code<'static> = c("111058", "Selected Region Description");
pub const SelectionStatus: Code<'static> = c("121404", "Selection Status");
pub const SemicircularSectors: Code<'static> = c("131301", "Semicircular sectors");
pub const SensitiveContent: Code<'static> = c("131395", "Sensitive Content");
pub const SentForAnalysis: Code<'static> = c("111457", "Sent for analysis");
pub const SeptalDefectIntervention: Code<'static> = c("122055", "Septal Defect Intervention");
pub const SeptalLineS: Code<'static> = c("112114", "Septal line(s)");
pub const SeptumToWall: Code<'static> = c("110876", "Septum To Wall");
pub const SeptumWallAxis: Code<'static> = c("110865", "Septum-Wall Axis");
pub const SequencedAcquisition: Code<'static> = c("113804", "Sequenced Acquisition");
pub const Series: Code<'static> = c("113015", "Series");
pub const SeriesDate: Code<'static> = c("131561", "Series Date");
pub const SeriesDescription: Code<'static> = c("131563", "Series Description");
pub const SeriesInstanceUID: Code<'static> = c("112002", "Series Instance UID");
pub const SeriesNumber: Code<'static> = c("113607", "Series Number");
pub const SeriesOrInstanceUsedForWaterEquivalentDiameterEstimation: Code<'static> = c("113985", "Series or Instance used for Water Equivalent Diameter estimation");
pub const SeriesTime: Code<'static> = c("131562", "Series Time");
pub const ServiceDeliveryLocation: Code<'static> = c("121434", "Service Delivery Location");
pub const ServicePerformer: Code<'static> = c("121435", "Service Performer");
pub const SessionStart: Code<'static> = c("110145", "Session start");
pub const SessionStop: Code<'static> = c("110146", "Session stop");
pub const SevereDistortionInTheAreaOfInterest: Code<'static> = c("130584", "Severe distortion in the area of interest");
pub const SeverelyJaundiced: Code<'static> = c("113563", "Severely Jaundiced");
pub const SeverityOfCalcification: Code<'static> = c("122132", "Severity of Calcification");
pub const SeverityOfComplication: Code<'static> = c("111466", "Severity of Complication");
pub const Sewing: Code<'static> = c("112319", "Sewing");
pub const SexOfAnimalsWithinSameHousingUnit: Code<'static> = c("127144", "Sex of animals within same housing unit");
pub const SexOfHandler: Code<'static> = c("127145", "Sex of handler");
pub const Shadow: Code<'static> = c("112124", "Shadow");
pub const ShammasNoHistory: Code<'static> = c("111874", "Shammas No-History");
pub const ShapeFromMotion: Code<'static> = c("114206", "Shape from motion");
pub const ShapeFromShading: Code<'static> = c("114205", "Shape from shading");
pub const SharplyDefined: Code<'static> = c("112137", "Sharply defined");
pub const SharplyDemarcated: Code<'static> = c("112140", "Sharply demarcated");
pub const ShearWaveDetectionMethod: Code<'static> = c("130759", "Shear Wave Detection Method");
pub const ShearWaveDispersionSlope: Code<'static> = c("130612", "Shear Wave Dispersion Slope");
pub const ShearWaveDispersionSlopeCenterFrequency: Code<'static> = c("130758", "Shear Wave Dispersion Slope Center Frequency");
pub const ShearWaveSpeed: Code<'static> = c("130611", "Shear Wave Speed");
pub const SheetFilmDigitized: Code<'static> = c("110020", "Sheet Film Digitized");
pub const ShelterManufacturer: Code<'static> = c("127196", "Shelter manufacturer");
pub const ShelterProductCode: Code<'static> = c("127198", "Shelter product code");
pub const ShelterProductName: Code<'static> = c("127197", "Shelter product name");
pub const ShelterType: Code<'static> = c("127195", "Shelter type");
pub const ShinRest: Code<'static> = c("130852", "Shin Rest");
pub const ShinRestPosition: Code<'static> = c("130843", "Shin Rest Position");
pub const ShortAxisViewAtPulmonaryArteryLevel: Code<'static> = c("131024", "Short axis view at pulmonary artery level");
pub const ShortLabel: Code<'static> = c("125309", "Short Label");
#[deprecated = "retired in PS3.16"]
pub const ShortRunHighGrayLevelEmphasis: Code<'static> = c("128806", "Short Run High Gray Level Emphasis");
#[deprecated = "retired in PS3.16"]
pub const ShortRunLowGrayLevelEmphasis: Code<'static> = c("128805", "Short Run Low Gray Level Emphasis");
#[deprecated = "retired in PS3.16"]
pub const ShortRunsEmphasis: Code<'static> = c("128801", "Short Runs Emphasis");
pub const ShutterSpeedModel: Code<'static> = c("126344", "Shutter-Speed Model");
pub const SideOfFamily: Code<'static> = c("111540", "Side of Family");
pub const SignalBaselineEnd: Code<'static> = c("122639", "Signal Baseline End");
pub const SignalBaselineStart: Code<'static> = c("122638", "Signal Baseline Start");
pub const SignalChange: Code<'static> = c("113059", "Signal Change");
pub const SignalEarliestPeakTime: Code<'static> = c("122631", "Signal Earliest Peak Time");
pub const SignalIncreaseStartTime: Code<'static> = c("122633", "Signal Increase Start Time");
pub const SignalTimeToPeak: Code<'static> = c("122634", "Signal Time to Peak");
pub const SignalToNoise: Code<'static> = c("113060", "Signal to Noise");
pub const SignalToNoiseRatio: Code<'static> = c("111787", "Signal to Noise Ratio");
pub const SignedCompleteAcquisitionContent: Code<'static> = c("113035", "Signed Complete Acquisition Content");
pub const SignedCompleteStudyContent: Code<'static> = c("113033", "Signed Complete Study Content");
pub const SignedManifest: Code<'static> = c("113031", "Signed Manifest");
pub const SignetRingSign: Code<'static> = c("112069", "Signet-ring sign");
pub const SilhouetteSign: Code<'static> = c("112152", "Silhouette sign");
pub const SiliconeGranuloma: Code<'static> = c("111296", "Silicone granuloma");
pub const SiliconeLadenLymphNodes: Code<'static> = c("111507", "Silicone-laden lymph nodes");
pub const SimpleObjectModel: Code<'static> = c("128418", "Simple Object Model");
pub const SimplifiedBernoulli: Code<'static> = c("125218", "Simplified Bernoulli");
pub const SimulatedKeratometry: Code<'static> = c("111755", "Simulated Keratometry");
pub const SimulationIntent: Code<'static> = c("129017", "Simulation Intent");
pub const SimulationNote: Code<'static> = c("130032", "Simulation Note");
pub const SimultaneousArterialPulseWaveform: Code<'static> = c("121306", "Simultaneous Arterial Pulse Waveform");
pub const SimultaneousDoppler: Code<'static> = c("121301", "Simultaneous Doppler");
pub const SimultaneousECG: Code<'static> = c("121303", "Simultaneous ECG");
pub const SimultaneousHemodynamic: Code<'static> = c("121302", "Simultaneous Hemodynamic");
pub const SimultaneousPhonocardiographicWaveform: Code<'static> = c("121307", "Simultaneous Phonocardiographic Waveform");
pub const SimultaneousRespiratoryWaveform: Code<'static> = c("121305", "Simultaneous Respiratory Waveform");
pub const SimultaneousVoiceNarrative: Code<'static> = c("121304", "Simultaneous Voice Narrative");
pub const SimultaneouslyAcquired: Code<'static> = c("122400", "Simultaneously Acquired");
pub const SingleFraction: Code<'static> = c("130096", "Single Fraction");
pub const SingleImageFinding: Code<'static> = c("111059", "Single Image Finding");
pub const SingleImageFindingModifier: Code<'static> = c("112024", "Single Image Finding Modifier");
pub const SingleLADRegionInRAOProjection: Code<'static> = c("122466", "Single LAD Region in RAO Projection");
pub const SingleLeaves: Code<'static> = c("130333", "Single Leaves");
pub const SingleLuminance: Code<'static> = c("111840", "Single luminance");
pub const SingleOrAnteriorLens: Code<'static> = c("111778", "Single or Anterior Lens");
#[deprecated = "retired in PS3.16"]
pub const SinglePhotonEmissionComputedTomography: Code<'static> = c("ST", "Single-photon emission computed tomography");
pub const SinglePlane: Code<'static> = c("113622", "Single Plane");
pub const SinglePlaneAngiography: Code<'static> = c("122059", "Single plane Angiography");
pub const SinglePlaneEllipse: Code<'static> = c("125226", "Single Plane Ellipse");
pub const SingleRCARegionInRAOProjection: Code<'static> = c("122467", "Single RCA Region in RAO Projection");
pub const SingleShotEPI: Code<'static> = c("113226", "Single Shot EPI");
pub const SingleSideIllumination: Code<'static> = c("130810", "Single-side illumination");
pub const SingleSubjectExtractedFromGroup: Code<'static> = c("113132", "Single subject extracted from group");
pub const SingleTensor: Code<'static> = c("113231", "Single Tensor");
pub const SingularValueDecompositionDeconvolution: Code<'static> = c("126311", "Singular Value Decomposition deconvolution");
pub const SiteInvolvement: Code<'static> = c("112008", "Site involvement");
#[deprecated = "retired in PS3.16"]
pub const SiteOfLumenMaximum: Code<'static> = c("122687", "Site of Lumen Maximum");
pub const SiteOfLuminalMaximum: Code<'static> = c("122516", "Site of Luminal Maximum");
pub const SiteOfLuminalMinimum: Code<'static> = c("122382", "Site of Luminal Minimum");
pub const SizeDescriptor: Code<'static> = c("112025", "Size Descriptor");
pub const SizeSpecificDoseEstimate: Code<'static> = c("113930", "Size Specific Dose Estimate");
pub const SizeSpecificDoseEstimateAtLongitudinalPositionZ: Code<'static> = c("113993", "Size Specific Dose Estimate At Longitudinal Position Z");
pub const Skewness: Code<'static> = c("126051", "Skewness");
pub const SkinAppositionSetupMethod: Code<'static> = c("130634", "Skin Apposition Setup Method");
pub const SkinConditionClammy: Code<'static> = c("122275", "Skin Condition Clammy");
pub const SkinConditionCold: Code<'static> = c("122273", "Skin Condition Cold");
pub const SkinConditionCool: Code<'static> = c("122272", "Skin Condition Cool");
pub const SkinConditionDiaphoretic: Code<'static> = c("122276", "Skin Condition Diaphoretic");
pub const SkinConditionDry: Code<'static> = c("122274", "Skin Condition Dry");
pub const SkinConditionFlush: Code<'static> = c("122277", "Skin Condition Flush");
pub const SkinConditionMottled: Code<'static> = c("122278", "Skin Condition Mottled");
pub const SkinConditionPale: Code<'static> = c("122279", "Skin Condition Pale");
pub const SkinConditionWarm: Code<'static> = c("122271", "Skin Condition Warm");
pub const SkinDoseMap: Code<'static> = c("128485", "Skin Dose Map");
pub const SkinLesionColor: Code<'static> = c("130832", "Skin lesion color");
pub const SkinOfAnteriorHelixOfEar: Code<'static> = c("130305", "Skin of anterior helix of ear");
pub const SkinOfCaruncleOfEye: Code<'static> = c("130306", "Skin of caruncle of eye");
pub const SkinOfInferiorHelixOfEar: Code<'static> = c("130307", "Skin of inferior helix of ear");
pub const SkinOfInferiorPosteriorSurfaceOfThePinna: Code<'static> = c("130308", "Skin of inferior posterior surface of the pinna");
pub const SkinOfInfraalarGroove: Code<'static> = c("130312", "Skin of infraalar groove");
pub const SkinOfLateralPartOfDorsumOfFoot: Code<'static> = c("130309", "Skin of lateral part of dorsum of foot");
pub const SkinOfLowerAntihelixOfEar: Code<'static> = c("130310", "Skin of lower antihelix of ear");
pub const SkinOfLowerEyelidMargin: Code<'static> = c("130311", "Skin of lower eyelid margin");
pub const SkinOfLowerParaspinalRegion: Code<'static> = c("130304", "Skin of lower paraspinal region");
pub const SkinOfMedialPartOfDorsumOfFoot: Code<'static> = c("130313", "Skin of medial part of dorsum of foot");
pub const SkinOfMidBack: Code<'static> = c("130323", "Skin of mid back");
pub const SkinOfMidParaspinalRegion: Code<'static> = c("130303", "Skin of mid paraspinal region");
pub const SkinOfParanasalCheek: Code<'static> = c("130314", "Skin of paranasal cheek");
pub const SkinOfParaspinalAreaOfTheNeck: Code<'static> = c("130300", "Skin of paraspinal area of the neck");
pub const SkinOfParaspinalAreaOfTheSuperiorBack: Code<'static> = c("130301", "Skin of paraspinal area of the superior back");
pub const SkinOfPosteriorHelixOfEar: Code<'static> = c("130315", "Skin of posterior helix of ear");
pub const SkinOfPosteriorLobuleOfTheEar: Code<'static> = c("130316", "Skin of posterior lobule of the ear");
pub const SkinOfSoleOfForefoot: Code<'static> = c("130317", "Skin of sole of forefoot");
pub const SkinOfSuperiorAntihelixOfEar: Code<'static> = c("130318", "Skin of superior antihelix of ear");
pub const SkinOfSuperiorPosteriorHelixOfEar: Code<'static> = c("130319", "Skin of superior posterior helix of ear");
pub const SkinOfSuperiorPosteriorSurfaceOfThePinna: Code<'static> = c("130320", "Skin of superior posterior surface of the pinna");
pub const SkinOfUpperAntihelixOfEar: Code<'static> = c("130321", "Skin of upper antihelix of ear");
pub const SkinOfUpperEyelidMargin: Code<'static> = c("130322", "Skin of upper eyelid margin");
pub const SkinOfUpperParaspinalRegion: Code<'static> = c("130302", "Skin of upper paraspinal region");
pub const SleepStageN1: Code<'static> = c("130834", "Sleep Stage N1");
pub const SleepStageN2: Code<'static> = c("130835", "Sleep Stage N2");
pub const SleepStageN3: Code<'static> = c("130836", "Sleep Stage N3");
pub const SliceContributionToEjectionFraction: Code<'static> = c("122498", "Slice Contribution to Ejection Fraction");
pub const SliceThickness: Code<'static> = c("112225", "Slice Thickness");
#[deprecated = "retired in PS3.16"]
pub const SlideIdentifier: Code<'static> = c("121043", "Slide Identifier");
pub const SlideMarking: Code<'static> = c("131511", "Slide marking");
pub const SlideMicroscopy_112702: Code<'static> = c("112702", "Slide Microscopy");
pub const SlideMicroscopy_SM: Code<'static> = c("SM", "Slide Microscopy");
#[deprecated = "retired in PS3.16"]
pub const SlideUID: Code<'static> = c("121044", "Slide UID");
pub const SlidingWindowBeam: Code<'static> = c("130106", "Sliding Window Beam");
pub const SlowDiffusionCoefficient: Code<'static> = c("113291", "Slow Diffusion Coefficient");
pub const SmallIrregularOpacities: Code<'static> = c("112125", "Small irregular opacities");
pub const SmallLessThan320CmLateralThickness: Code<'static> = c("113601", "Small: < 32.0 cm lateral thickness");
pub const SmallRoundedOpacities: Code<'static> = c("112126", "Small rounded opacities");
#[deprecated = "retired in PS3.16"]
pub const SmallZoneEmphasis: Code<'static> = c("128821", "Small Zone Emphasis");
#[deprecated = "retired in PS3.16"]
pub const SmallZoneHighGrayLevelEmphasis: Code<'static> = c("128826", "Small Zone High Gray Level Emphasis");
#[deprecated = "retired in PS3.16"]
pub const SmallZoneLowGrayLevelEmphasis: Code<'static> = c("128825", "Small Zone Low Gray Level Emphasis");
pub const Smoothing: Code<'static> = c("113087", "Smoothing");
pub const SoftPlaque: Code<'static> = c("122356", "Soft plaque");
#[deprecated = "retired in PS3.16"]
pub const SoftTissue: Code<'static> = c("112144", "Soft tissue");
pub const SoftwareConfiguration: Code<'static> = c("110131", "Software Configuration");
pub const SoftwareFailure: Code<'static> = c("111218", "Software failure");
pub const SolidMass: Code<'static> = c("111462", "Solid mass");
pub const Sonazoid: Code<'static> = c("125905", "Sonazoid");
pub const SonoVue: Code<'static> = c("125906", "SonoVue");
#[deprecated = "retired in PS3.16"]
pub const Sonographer: Code<'static> = c("121093", "Sonographer");
pub const Sonologist: Code<'static> = c("121092", "Sonologist");
pub const SourceContinuousRollAngle: Code<'static> = c("130798", "Source Continuous Roll Angle");
pub const SourceDeformableSpatialRegistration: Code<'static> = c("125028", "Source Deformable Spatial Registration");
pub const SourceDocument: Code<'static> = c("121335", "Source Document");
pub const SourceDoseForComposingCurrentDose: Code<'static> = c("121372", "Source dose for composing current dose");
pub const SourceImage: Code<'static> = c("121324", "Source Image");
pub const SourceImageDiffusionBValue: Code<'static> = c("113240", "Source image diffusion b-value");
pub const SourceImageForImageProcessingOperation: Code<'static> = c("121322", "Source image for image processing operation");
pub const SourceImageForMontage: Code<'static> = c("121329", "Source image for montage");
pub const SourceImageForSegmentation: Code<'static> = c("121233", "Source image for segmentation");
pub const SourceMeasurement: Code<'static> = c("128224", "Source measurement");
pub const SourceMedia: Code<'static> = c("110155", "Source Media");
pub const SourceOfDoseInformation: Code<'static> = c("113854", "Source of Dose Information");
pub const SourceOfMeasurement: Code<'static> = c("121112", "Source of Measurement");
pub const SourcePositionToleranceViolation: Code<'static> = c("130473", "Source Position Tolerance Violation");
pub const SourceRawData: Code<'static> = c("128226", "Source raw data");
pub const SourceRealWorldValueMap: Code<'static> = c("128227", "Source real world value map");
pub const SourceReport: Code<'static> = c("128225", "Source report");
pub const SourceRoleID: Code<'static> = c("110153", "Source Role ID");
pub const SourceSegmentation: Code<'static> = c("128228", "Source segmentation");
pub const SourceSeriesForImageProcessingOperation: Code<'static> = c("121323", "Source series for image processing operation");
pub const SourceSeriesForSegmentation: Code<'static> = c("121232", "Source series for segmentation");
pub const SpacingBetweenSlices: Code<'static> = c("112226", "Spacing between slices");
pub const SpatialDisplacementXComponent: Code<'static> = c("110822", "Spatial Displacement X Component");
pub const SpatialDisplacementYComponent: Code<'static> = c("110823", "Spatial Displacement Y Component");
pub const SpatialDisplacementZComponent: Code<'static> = c("110824", "Spatial Displacement Z Component");
pub const SpatialFiducials_128447: Code<'static> = c("128447", "Spatial Fiducials");
pub const SpatialFiducials_FID: Code<'static> = c("FID", "Spatial Fiducials");
pub const SpatialRegistration: Code<'static> = c("112353", "Spatial Registration");
pub const SpatialRegistrationReference: Code<'static> = c("128444", "Spatial Registration Reference");
pub const SpatialResampling: Code<'static> = c("113085", "Spatial resampling");
pub const SpatiallyRelatedFramesExtractedFromTheVolume: Code<'static> = c("113091", "Spatially-related frames extracted from the volume");
pub const SpatiallyTransformedDose: Code<'static> = c("131406", "Spatially Transformed Dose");
pub const SpecialProcedureNote: Code<'static> = c("130025", "Special Procedure Note");
pub const SpecificAbsorptionRate: Code<'static> = c("131402", "Specific Absorption Rate");
pub const SpecificObservationSubjectClass: Code<'static> = c("130780", "Specific observation subject class");
pub const Specified: Code<'static> = c("131232", "Specified");
pub const SpecifiedConformationNumber: Code<'static> = c("130076", "Specified Conformation Number");
pub const SpecifiedConformityIndex: Code<'static> = c("130074", "Specified Conformity Index");
pub const SpecifiedHealthyTissueConformityIndex: Code<'static> = c("130075", "Specified Healthy Tissue Conformity Index");
pub const SpecifiedHomogeneityIndex: Code<'static> = c("130077", "Specified Homogeneity Index");
pub const SpecifiedRadiationDose: Code<'static> = c("130019", "Specified Radiation Dose");
pub const SpecifiedVolumePercentage: Code<'static> = c("130021", "Specified Volume Percentage");
pub const SpecifiedVolumeSize: Code<'static> = c("130020", "Specified Volume Size");
pub const Specimen: Code<'static> = c("121027", "Specimen");
#[deprecated = "retired in PS3.16"]
pub const SpecimenAccessionNumber: Code<'static> = c("121040", "Specimen Accession Number");
pub const SpecimenContainerIdentifier: Code<'static> = c("111700", "Specimen Container Identifier");
pub const SpecimenIdentifier: Code<'static> = c("121041", "Specimen Identifier");
pub const SpecimenStorage: Code<'static> = c("111729", "Specimen storage");
#[deprecated = "retired in PS3.16"]
pub const SpecimenType: Code<'static> = c("121042", "Specimen Type");
pub const SpecimenUID: Code<'static> = c("121039", "Specimen UID");
pub const SpectaclePrescriptionReport: Code<'static> = c("111671", "Spectacle Prescription Report");
pub const SpectralDomain: Code<'static> = c("111921", "Spectral domain");
pub const SpectralFilteredImage: Code<'static> = c("121340", "Spectral filtered image");
pub const Spectroscopy: Code<'static> = c("121317", "Spectroscopy");
pub const SpectroscopyDataForWaterPhaseCorrection: Code<'static> = c("121318", "Spectroscopy Data for Water Phase Correction");
pub const SpeedOfSound: Code<'static> = c("110832", "Speed of sound");
pub const SpeedOfSoundMapCorrection: Code<'static> = c("130820", "Speed of Sound Map Correction");
pub const Sphere: Code<'static> = c("122485", "Sphere");
pub const SphericalBackProjection: Code<'static> = c("130821", "Spherical Back Projection");
pub const SphericalDeconvolution: Code<'static> = c("113238", "Spherical Deconvolution");
pub const SphericalProjection: Code<'static> = c("111791", "Spherical projection");
pub const Spiculated: Code<'static> = c("112136", "Spiculated");
pub const SpinTaggingPerfusionMRSignalIntensity: Code<'static> = c("110800", "Spin Tagging Perfusion MR Signal Intensity");
pub const SpontaneousAbortion: Code<'static> = c("111566", "Spontaneous Abortion");
pub const SpotMagnificationViewS: Code<'static> = c("111136", "Spot magnification view(s)");
pub const SquamousCellCarcinomaOfTheNipple: Code<'static> = c("111340", "Squamous cell carcinoma of the nipple");
pub const Stage0: Code<'static> = c("111494", "Stage 0");
pub const StageI: Code<'static> = c("111495", "Stage I");
pub const StageIIA: Code<'static> = c("111496", "Stage IIA");
pub const StageIIB: Code<'static> = c("111497", "Stage IIB");
pub const StageIIIA: Code<'static> = c("111498", "Stage IIIA");
pub const StageIIIB: Code<'static> = c("111499", "Stage IIIB");
pub const StageIIIC: Code<'static> = c("111500", "Stage IIIC");
pub const StageIV: Code<'static> = c("111501", "Stage IV");
pub const StageView: Code<'static> = c("113017", "Stage-View");
pub const StagedMeasurements: Code<'static> = c("125310", "Staged Measurements");
pub const StagingPriorToImaging: Code<'static> = c("127103", "Staging prior to imaging");
pub const StandardDeviation: Code<'static> = c("113061", "Standard Deviation");
pub const StandardDeviationDistanceOfCalcificationsInACluster: Code<'static> = c("112201", "Standard deviation distance of calcifications in a cluster");
pub const StandardDeviationOfAttenuationCoefficient: Code<'static> = c("112183", "Standard Deviation of Attenuation Coefficient");
pub const StandardDeviationOfMeasurementsUsed: Code<'static> = c("111786", "Standard Deviation of measurements used");
pub const StandardDeviationOfPopulation: Code<'static> = c("121414", "Standard deviation of population");
pub const StandardDeviationOfUltrasoundAttenuationCoefficient: Code<'static> = c("131344", "Standard Deviation of Ultrasound Attenuation Coefficient");
pub const StandardFractionation: Code<'static> = c("130097", "Standard Fractionation");
pub const StandardToftsModel: Code<'static> = c("126340", "Standard Tofts Model");
pub const StandardizedAddedMetabolicActivity: Code<'static> = c("126037", "Standardized Added Metabolic Activity");
pub const StandardizedAddedMetabolicActivityBackground: Code<'static> = c("126038", "Standardized Added Metabolic Activity Background");
pub const StandardizedUptakeValue: Code<'static> = c("126400", "Standardized Uptake Value");
pub const Standby: Code<'static> = c("121101", "Standby");
pub const StartOfAtrialContraction: Code<'static> = c("109045", "Start of atrial contraction");
pub const StartOfAtrialContractionSubsequent: Code<'static> = c("109046", "Start of atrial contraction (subsequent)");
pub const StartOfContrastAgentAdministration: Code<'static> = c("125234", "Start of contrast agent administration");
pub const StartOfDrugDoseAdministration: Code<'static> = c("125233", "Start of drug dose administration");
pub const StartOfExpiration: Code<'static> = c("109029", "Start of expiration");
pub const StartOfInspiration: Code<'static> = c("109030", "Start of inspiration");
pub const StartOfThermalCO: Code<'static> = c("109031", "Start of thermal CO");
pub const StartOfXRayIrradiation: Code<'static> = c("113809", "Start of X-Ray Irradiation");
pub const StartProcedureAction: Code<'static> = c("121130", "Start Procedure Action");
pub const StartingFlowRateOfAdministration: Code<'static> = c("130208", "Starting Flow Rate of administration");
pub const Statement: Code<'static> = c("122152", "Statement");
pub const StatementModifier: Code<'static> = c("122153", "Statement Modifier");
pub const StaticBeam: Code<'static> = c("130102", "Static Beam");
pub const StationAETitle: Code<'static> = c("110119", "Station AE Title");
pub const StationaryAcquisition_113611: Code<'static> = c("113611", "Stationary Acquisition");
pub const StationaryAcquisition_113806: Code<'static> = c("113806", "Stationary Acquisition");
pub const StenoticFlowReserve: Code<'static> = c("122548", "Stenotic Flow Reserve");
pub const StentComposition: Code<'static> = c("122685", "Stent Composition");
pub const StentDiameterRatio: Code<'static> = c("122351", "Stent Diameter Ratio");
pub const StentDisintegration: Code<'static> = c("122684", "Stent Disintegration");
pub const StentExpansionIndex: Code<'static> = c("122347", "Stent Expansion Index");
pub const StentFracture: Code<'static> = c("122683", "Stent Fracture");
pub const StentGap: Code<'static> = c("122364", "Stent Gap");
pub const StentReExpanded: Code<'static> = c("122306", "Stent re-expanded");
pub const StentSymmetryIndex: Code<'static> = c("122346", "Stent Symmetry Index");
pub const StentVolumeObstruction: Code<'static> = c("122339", "Stent Volume Obstruction");
pub const StentedRegion: Code<'static> = c("122384", "Stented Region");
pub const StepAndShootBeam: Code<'static> = c("130105", "Step and Shoot Beam");
pub const SteppingAcquisition: Code<'static> = c("113612", "Stepping Acquisition");
pub const StereometricRelationship: Code<'static> = c("SMR", "Stereometric Relationship");
pub const StereotacticCoordinates: Code<'static> = c("127450", "Stereotactic coordinates");
pub const StereotacticSetupMethod: Code<'static> = c("130633", "Stereotactic Setup Method");
pub const StimulationAtRate1Interval: Code<'static> = c("109047", "Stimulation at rate 1 interval");
pub const StimulationAtRate2Interval: Code<'static> = c("109048", "Stimulation at rate 2 interval");
pub const StimulationAtRate3Interval: Code<'static> = c("109049", "Stimulation at rate 3 interval");
pub const StimulationAtRate4Interval: Code<'static> = c("109050", "Stimulation at rate 4 interval");
pub const StimulationMode: Code<'static> = c("130491", "Stimulation Mode");
pub const StimulusSamplePosition: Code<'static> = c("130492", "Stimulus Sample Position");
pub const StimulusTimeOffset: Code<'static> = c("130493", "Stimulus Time Offset");
pub const Stitching: Code<'static> = c("131512", "Stitching");
pub const StorageDetector: Code<'static> = c("113950", "Storage Detector");
pub const Strain: Code<'static> = c("127411", "Strain");
pub const StrainDescription: Code<'static> = c("127412", "Strain description");
pub const Streamline: Code<'static> = c("113215", "Streamline");
pub const StressProtocol: Code<'static> = c("109056", "Stress Protocol");
pub const StressTechnetiumPerRestTechnetium1DayProcedure: Code<'static> = c("122784", "Stress technetium/rest technetium 1 day procedure");
pub const StressTestScore: Code<'static> = c("122760", "Stress test score");
pub const StretchedExponentialDiffusionModel: Code<'static> = c("113254", "Stretched exponential diffusion model");
pub const StripFilter: Code<'static> = c("113650", "Strip filter");
pub const Stripe: Code<'static> = c("112094", "Stripe");
pub const Stripes: Code<'static> = c("131513", "Stripes");
pub const StronglyPositive: Code<'static> = c("122755", "Strongly positive");
pub const StronglyPositiveSTElevation: Code<'static> = c("122756", "Strongly positive - ST elevation");
pub const StructuralImageForImageProcessing: Code<'static> = c("128250", "Structural image for image processing");
pub const StructureOfTheFindingSite: Code<'static> = c("125311", "Structure of the Finding Site");
pub const StructuredReportDocument: Code<'static> = c("SR", "Structured Report Document");
pub const StudentsTTest: Code<'static> = c("113068", "Student's T-Test");
pub const StudiesImported: Code<'static> = c("110027", "Studies Imported");
pub const Study: Code<'static> = c("113014", "Study");
pub const StudyDate: Code<'static> = c("111060", "Study Date");
pub const StudyInstanceUID: Code<'static> = c("110180", "Study Instance UID");
pub const StudyPerformedDidNotMatchRequest: Code<'static> = c("130573", "Study performed did not match request");
pub const StudyTime: Code<'static> = c("111061", "Study Time");
pub const SubjectAge: Code<'static> = c("121033", "Subject Age");
pub const SubjectBirthDate: Code<'static> = c("121031", "Subject Birth Date");
pub const SubjectBreed: Code<'static> = c("121035", "Subject Breed");
pub const SubjectClass: Code<'static> = c("121024", "Subject Class");
pub const SubjectID: Code<'static> = c("121030", "Subject ID");
pub const SubjectName: Code<'static> = c("121029", "Subject Name");
pub const SubjectSex: Code<'static> = c("121032", "Subject Sex");
pub const SubjectSexParametersForClinicalUseCategory: Code<'static> = c("131233", "Subject Sex Parameters for Clinical Use Category");
pub const SubjectSexParametersForClinicalUseCategoryComment: Code<'static> = c("131234", "Subject Sex Parameters for Clinical Use Category Comment");
pub const SubjectSexParametersForClinicalUseCategoryReference: Code<'static> = c("131235", "Subject Sex Parameters for Clinical Use Category Reference");
pub const SubjectSpecies: Code<'static> = c("121034", "Subject Species");
pub const SubjectTimePointIdentifier: Code<'static> = c("126070", "Subject Time Point Identifier");
pub const SubjectUID: Code<'static> = c("121028", "Subject UID");
pub const SubjectiveRefraction: Code<'static> = c("SRF", "Subjective Refraction");
pub const SuboptimalContrastTiming: Code<'static> = c("128554", "Suboptimal contrast timing");
pub const SuboptimalPatientPreparation: Code<'static> = c("130575", "Suboptimal patient preparation");
#[deprecated = "retired in PS3.16"]
pub const Subpleural: Code<'static> = c("112153", "Subpleural");
pub const SubpleuralLine: Code<'static> = c("112115", "Subpleural line");
pub const SubscapularFossa: Code<'static> = c("112098", "Subscapular Fossa");
pub const SubstanceUseHistory: Code<'static> = c("111545", "Substance Use History");
pub const SubtractionImageMissing: Code<'static> = c("130581", "Subtraction image missing");
pub const Succeeded: Code<'static> = c("111222", "Succeeded");
pub const SuccessfulAnalyses: Code<'static> = c("111062", "Successful Analyses");
pub const SuccessfulDetections: Code<'static> = c("111063", "Successful Detections");
pub const SuggestiveOfMalignancyTakeAppropriateAction: Code<'static> = c("111146", "Suggestive of malignancy - take appropriate action");
#[deprecated = "retired in PS3.16"]
pub const SumAverageOfGLCM: Code<'static> = c("128787", "Sum Average of GLCM");
#[deprecated = "retired in PS3.16"]
pub const SumEntropyOfGLCM: Code<'static> = c("128789", "Sum Entropy of GLCM");
pub const SumOfSegmentedVoxelMethodForVolume: Code<'static> = c("126030", "Sum of segmented voxel method for volume");
#[deprecated = "retired in PS3.16"]
pub const SumVarianceOfGLCM: Code<'static> = c("128788", "Sum Variance of GLCM");
#[deprecated = "retired in PS3.16"]
pub const Summary: Code<'static> = c("121111", "Summary");
pub const SummaryOfAnalyses: Code<'static> = c("111065", "Summary of Analyses");
pub const SummaryOfDetections: Code<'static> = c("111064", "Summary of Detections");
pub const SummationProjection: Code<'static> = c("130926", "Summation projection");
pub const SuperficialRetinaStructuralReflectanceMap: Code<'static> = c("128266", "Superficial retina structural reflectance map");
pub const SuperficialRetinaVasculatureFlow: Code<'static> = c("128265", "Superficial retina vasculature flow");
pub const SuperficialVascularPlexusFlow: Code<'static> = c("128308", "Superficial vascular plexus flow");
pub const SuperficialVascularPlexusMap: Code<'static> = c("128309", "Superficial vascular plexus map");
pub const SuperiorLongitudinalFasciculusI: Code<'static> = c("110703", "superior longitudinal fasciculus I");
pub const SuperiorLongitudinalFasciculusII: Code<'static> = c("110704", "superior longitudinal fasciculus II");
pub const SuperiorLongitudinalFasciculusIII: Code<'static> = c("110705", "superior longitudinal fasciculus III");
pub const SuperolateralToInferomedial: Code<'static> = c("111434", "Superolateral to inferomedial");
pub const SuperomedialToInferolateral: Code<'static> = c("111435", "Superomedial to inferolateral");
pub const SupplementaryData: Code<'static> = c("111414", "Supplementary Data");
pub const SupplementaryDataForIntervention: Code<'static> = c("111463", "Supplementary Data for Intervention");
pub const SupportingInformation: Code<'static> = c("112359", "Supporting Information");
pub const SurfaceBetweenInnerAndOuterSegmentsOfThePhotoreceptors: Code<'static> = c("128295", "Surface between Inner and Outer Segments of the photoreceptors");
pub const SurfaceContourMapping: Code<'static> = c("111792", "Surface contour mapping");
pub const SurfaceOfTheCenterOfTheRPE: Code<'static> = c("128298", "Surface of the center of the RPE");
pub const SurfaceOfTheChoroidScleraInterface: Code<'static> = c("128301", "Surface of the choroid-sclera interface");
pub const SurfaceOfTheInterdigitationZoneBetweenRetinaAndRPE: Code<'static> = c("128296", "Surface of the interdigitation zone between retina and RPE");
pub const SurfaceRendering: Code<'static> = c("113075", "Surface rendering");
#[deprecated = "retired in PS3.16"]
pub const Surgeon: Code<'static> = c("121091", "Surgeon");
pub const SurgeonFactor: Code<'static> = c("111773", "Surgeon Factor");
pub const SurgicalConsult: Code<'static> = c("111410", "Surgical consult");
pub const Survey: Code<'static> = c("111128", "Survey");
pub const SuspendProcedureAction: Code<'static> = c("121132", "Suspend Procedure Action");
pub const SynchronizationFrameOfReferenceUID: Code<'static> = c("130885", "Synchronization Frame of Reference UID");
pub const SynchronizedRoboticTreatment: Code<'static> = c("130139", "Synchronized Robotic Treatment");
pub const SynthesizingEquipment: Code<'static> = c("109100", "Synthesizing Equipment");
pub const SyringeAttached: Code<'static> = c("130163", "Syringe attached");
pub const SyringeDetached: Code<'static> = c("130164", "Syringe detached");
pub const SystemCalculated: Code<'static> = c("113940", "System Calculated");
pub const SystemicFlow: Code<'static> = c("122162", "Systemic Flow");
pub const SystolicBloodVelocityMean: Code<'static> = c("122203", "Systolic blood velocity, mean");
pub const SystolicBloodVelocityPeak: Code<'static> = c("122204", "Systolic blood velocity, peak");
#[deprecated = "retired in PS3.16"]
pub const SystolicPeakPressure: Code<'static> = c("109033", "Systolic peak pressure");
#[deprecated = "retired in PS3.16"]
pub const SystolicPressureAverage: Code<'static> = c("109032", "Systolic pressure, average");
pub const T1: Code<'static> = c("113063", "T1");
pub const T1ByFixedValue: Code<'static> = c("126352", "T1 by Fixed Value");
pub const T1ByInversionRecovery: Code<'static> = c("126351", "T1 by Inversion Recovery");
pub const T1ByMultipleFlipAngles: Code<'static> = c("126350", "T1 by Multiple Flip Angles");
pub const T1UsedForCalculation: Code<'static> = c("126353", "T1 Used For Calculation");
pub const T1WeightedDynamicContrastEnhancedMRSignalIntensity: Code<'static> = c("110816", "T1 Weighted Dynamic Contrast Enhanced MR Signal Intensity");
pub const T1WeightedMRSignalIntensity: Code<'static> = c("110804", "T1 Weighted MR Signal Intensity");
pub const T1Worst: Code<'static> = c("122367", "T-1 Worst");
pub const T2Secondary: Code<'static> = c("122368", "T-2 Secondary");
pub const T2WeightedDynamicContrastEnhancedMRSignalIntensity_110817: Code<'static> = c("110817", "T2 Weighted Dynamic Contrast Enhanced MR Signal Intensity");
pub const T2WeightedDynamicContrastEnhancedMRSignalIntensity_110818: Code<'static> = c("110818", "T2* Weighted Dynamic Contrast Enhanced MR Signal Intensity");
pub const T2WeightedMRSignalIntensity_110805: Code<'static> = c("110805", "T2 Weighted MR Signal Intensity");
pub const T2WeightedMRSignalIntensity_110806: Code<'static> = c("110806", "T2* Weighted MR Signal Intensity");
pub const T2_113064: Code<'static> = c("113064", "T2*");
pub const T2_113065: Code<'static> = c("113065", "T2");
pub const T3Secondary: Code<'static> = c("122369", "T-3 Secondary");
pub const T4Secondary: Code<'static> = c("122370", "T-4 Secondary");
pub const T807F18: Code<'static> = c("126502", "T807 F^18^");
pub const TBISetupMethod: Code<'static> = c("130632", "TBI Setup Method");
pub const TEND: Code<'static> = c("113216", "TEND");
pub const TG18ADPattern: Code<'static> = c("109845", "TG18-AD Pattern");
pub const TG18AFCPattern: Code<'static> = c("109861", "TG18-AFC Pattern");
pub const TG18BRPattern: Code<'static> = c("109802", "TG18-BR Pattern");
pub const TG18CHImage: Code<'static> = c("109878", "TG18-CH Image");
pub const TG18CTPattern: Code<'static> = c("109804", "TG18-CT Pattern");
pub const TG18CXPattern: Code<'static> = c("109854", "TG18-CX Pattern");
pub const TG18GA03Pattern: Code<'static> = c("109870", "TG18-GA03 Pattern");
pub const TG18GA05Pattern: Code<'static> = c("109871", "TG18-GA05 Pattern");
pub const TG18GA08Pattern: Code<'static> = c("109872", "TG18-GA08 Pattern");
pub const TG18GA10Pattern: Code<'static> = c("109873", "TG18-GA10 Pattern");
pub const TG18GA15Pattern: Code<'static> = c("109874", "TG18-GA15 Pattern");
pub const TG18GA20Pattern: Code<'static> = c("109875", "TG18-GA20 Pattern");
pub const TG18GA25Pattern: Code<'static> = c("109876", "TG18-GA25 Pattern");
pub const TG18GA30Pattern: Code<'static> = c("109877", "TG18-GA30 Pattern");
pub const TG18GQBPattern: Code<'static> = c("109869", "TG18-GQB Pattern");
pub const TG18GQNPattern: Code<'static> = c("109868", "TG18-GQN Pattern");
pub const TG18GQPattern: Code<'static> = c("109867", "TG18-GQ Pattern");
pub const TG18GVNPattern: Code<'static> = c("109866", "TG18-GVN Pattern");
pub const TG18GVPattern: Code<'static> = c("109865", "TG18-GV Pattern");
pub const TG18KNImage: Code<'static> = c("109879", "TG18-KN Image");
pub const TG18LN1201Pattern: Code<'static> = c("109823", "TG18-LN12-01 Pattern");
pub const TG18LN1202Pattern: Code<'static> = c("109824", "TG18-LN12-02 Pattern");
pub const TG18LN1203Pattern: Code<'static> = c("109825", "TG18-LN12-03 Pattern");
pub const TG18LN1204Pattern: Code<'static> = c("109826", "TG18-LN12-04 Pattern");
pub const TG18LN1205Pattern: Code<'static> = c("109827", "TG18-LN12-05 Pattern");
pub const TG18LN1206Pattern: Code<'static> = c("109828", "TG18-LN12-06 Pattern");
pub const TG18LN1207Pattern: Code<'static> = c("109829", "TG18-LN12-07 Pattern");
pub const TG18LN1208Pattern: Code<'static> = c("109830", "TG18-LN12-08 Pattern");
pub const TG18LN1209Pattern: Code<'static> = c("109831", "TG18-LN12-09 Pattern");
pub const TG18LN1210Pattern: Code<'static> = c("109832", "TG18-LN12-10 Pattern");
pub const TG18LN1211Pattern: Code<'static> = c("109833", "TG18-LN12-11 Pattern");
pub const TG18LN1212Pattern: Code<'static> = c("109834", "TG18-LN12-12 Pattern");
pub const TG18LN1213Pattern: Code<'static> = c("109835", "TG18-LN12-13 Pattern");
pub const TG18LN1214Pattern: Code<'static> = c("109836", "TG18-LN12-14 Pattern");
pub const TG18LN1215Pattern: Code<'static> = c("109837", "TG18-LN12-15 Pattern");
pub const TG18LN1216Pattern: Code<'static> = c("109838", "TG18-LN12-16 Pattern");
pub const TG18LN1217Pattern: Code<'static> = c("109839", "TG18-LN12-17 Pattern");
pub const TG18LN1218Pattern: Code<'static> = c("109840", "TG18-LN12-18 Pattern");
pub const TG18LN801Pattern: Code<'static> = c("109805", "TG18-LN8-01 Pattern");
pub const TG18LN802Pattern: Code<'static> = c("109806", "TG18-LN8-02 Pattern");
pub const TG18LN803Pattern: Code<'static> = c("109807", "TG18-LN8-03 Pattern");
pub const TG18LN804Pattern: Code<'static> = c("109808", "TG18-LN8-04 Pattern");
pub const TG18LN805Pattern: Code<'static> = c("109809", "TG18-LN8-05 Pattern");
pub const TG18LN806Pattern: Code<'static> = c("109810", "TG18-LN8-06 Pattern");
pub const TG18LN807Pattern: Code<'static> = c("109811", "TG18-LN8-07 Pattern");
pub const TG18LN808Pattern: Code<'static> = c("109812", "TG18-LN8-08 Pattern");
pub const TG18LN809Pattern: Code<'static> = c("109813", "TG18-LN8-09 Pattern");
pub const TG18LN810Pattern: Code<'static> = c("109814", "TG18-LN8-10 Pattern");
pub const TG18LN811Pattern: Code<'static> = c("109815", "TG18-LN8-11 Pattern");
pub const TG18LN812Pattern: Code<'static> = c("109816", "TG18-LN8-12 Pattern");
pub const TG18LN813Pattern: Code<'static> = c("109817", "TG18-LN8-13 Pattern");
pub const TG18LN814Pattern: Code<'static> = c("109818", "TG18-LN8-14 Pattern");
pub const TG18LN815Pattern: Code<'static> = c("109819", "TG18-LN8-15 Pattern");
pub const TG18LN816Pattern: Code<'static> = c("109820", "TG18-LN8-16 Pattern");
pub const TG18LN817Pattern: Code<'static> = c("109821", "TG18-LN8-17 Pattern");
pub const TG18LN818Pattern: Code<'static> = c("109822", "TG18-LN8-18 Pattern");
pub const TG18LPH10Pattern: Code<'static> = c("109855", "TG18-LPH10 Pattern");
pub const TG18LPH50Pattern: Code<'static> = c("109856", "TG18-LPH50 Pattern");
pub const TG18LPH89Pattern: Code<'static> = c("109857", "TG18-LPH89 Pattern");
pub const TG18LPV10Pattern: Code<'static> = c("109858", "TG18-LPV10 Pattern");
pub const TG18LPV50Pattern: Code<'static> = c("109859", "TG18-LPV50 Pattern");
pub const TG18LPV89Pattern: Code<'static> = c("109860", "TG18-LPV89 Pattern");
pub const TG18MM1Image: Code<'static> = c("109880", "TG18-MM1 Image");
pub const TG18MM2Image: Code<'static> = c("109881", "TG18-MM2 Image");
pub const TG18MPPattern: Code<'static> = c("109846", "TG18-MP Pattern");
pub const TG18NS10Pattern: Code<'static> = c("109862", "TG18-NS10 Pattern");
pub const TG18NS50Pattern: Code<'static> = c("109863", "TG18-NS50 Pattern");
pub const TG18NS89Pattern: Code<'static> = c("109864", "TG18-NS89 Pattern");
pub const TG18PQCPattern: Code<'static> = c("109803", "TG18-PQC Pattern");
pub const TG18PXPattern: Code<'static> = c("109853", "TG18-PX Pattern");
pub const TG18QCPattern: Code<'static> = c("109801", "TG18-QC Pattern");
pub const TG18RH10Pattern: Code<'static> = c("109847", "TG18-RH10 Pattern");
pub const TG18RH50Pattern: Code<'static> = c("109848", "TG18-RH50 Pattern");
pub const TG18RH89Pattern: Code<'static> = c("109849", "TG18-RH89 Pattern");
pub const TG18RV10Pattern: Code<'static> = c("109850", "TG18-RV10 Pattern");
pub const TG18RV50Pattern: Code<'static> = c("109851", "TG18-RV50 Pattern");
pub const TG18RV89Pattern: Code<'static> = c("109852", "TG18-RV89 Pattern");
pub const TG18UN10Pattern: Code<'static> = c("109841", "TG18-UN10 Pattern");
pub const TG18UN80Pattern: Code<'static> = c("109842", "TG18-UN80 Pattern");
pub const TG18UNL10Pattern: Code<'static> = c("109843", "TG18-UNL10 Pattern");
pub const TG18UNL80Pattern: Code<'static> = c("109844", "TG18-UNL80 Pattern");
#[deprecated = "retired in PS3.16"]
pub const THK5351F18: Code<'static> = c("126717", "THK5351 F^18^");
pub const TMRAndOARRatios: Code<'static> = c("131322", "TMR and OAR Ratios");
pub const TRC10589Zr: Code<'static> = c("126749", "TRC105 ^89^Zr");
pub const TSPyramid: Code<'static> = c("122564", "TS Pyramid");
pub const TWave: Code<'static> = c("109051", "T wave");
pub const Table: Code<'static> = c("128459", "Table");
pub const TableCore: Code<'static> = c("128460", "Table Core");
pub const TableCradleTiltAngle: Code<'static> = c("113756", "Table Cradle Tilt Angle");
pub const TableCradleTiltEndAngle: Code<'static> = c("128765", "Table Cradle Tilt End Angle");
pub const TableHeadTiltAngle: Code<'static> = c("113754", "Table Head Tilt Angle");
pub const TableHeadTiltEndAngle: Code<'static> = c("128763", "Table Head Tilt End Angle");
pub const TableHeightEndPosition: Code<'static> = c("113761", "Table Height End Position");
pub const TableHeightPosition: Code<'static> = c("113753", "Table Height Position");
pub const TableHorizontalRotationAngle: Code<'static> = c("113755", "Table Horizontal Rotation Angle");
pub const TableHorizontalRotationEndAngle: Code<'static> = c("128764", "Table Horizontal Rotation End Angle");
pub const TableLateralEndPosition: Code<'static> = c("113760", "Table Lateral End Position");
pub const TableLateralPosition: Code<'static> = c("113752", "Table Lateral Position");
pub const TableLongitudinalEndPosition: Code<'static> = c("113759", "Table Longitudinal End Position");
pub const TableLongitudinalPosition: Code<'static> = c("113751", "Table Longitudinal Position");
pub const TableMount: Code<'static> = c("113952", "Table Mount");
pub const TableOfValues: Code<'static> = c("121424", "Table of Values");
pub const TableOfValuesCitation: Code<'static> = c("121422", "Table of Values Citation");
pub const TableOuterLiner: Code<'static> = c("128461", "Table Outer Liner");
pub const TablePad: Code<'static> = c("128462", "Table Pad");
pub const TableXEndPositionToIsocenter: Code<'static> = c("128769", "Table X End Position to Isocenter");
pub const TableXPositionToIsocenter: Code<'static> = c("128766", "Table X Position to Isocenter");
pub const TableYEndPositionToIsocenter: Code<'static> = c("128770", "Table Y End Position to Isocenter");
pub const TableYPositionToIsocenter: Code<'static> = c("128767", "Table Y Position to Isocenter");
pub const TableZEndPositionToIsocenter: Code<'static> = c("128771", "Table Z End Position to Isocenter");
pub const TableZPositionToIsocenter: Code<'static> = c("128768", "Table Z Position to Isocenter");
pub const TabularDataAlgorithm: Code<'static> = c("128479", "Tabular Data Algorithm");
pub const TapTestBegin: Code<'static> = c("130895", "Tap test begin");
pub const TapTestEnd: Code<'static> = c("130896", "Tap test end");
pub const TargestarB: Code<'static> = c("125907", "Targestar-B");
pub const TargestarP: Code<'static> = c("125908", "Targestar-P");
pub const Target: Code<'static> = c("112162", "Target");
pub const TargetCompletelyContainedInTheSpecimen: Code<'static> = c("111443", "Target completely contained in the specimen");
pub const TargetContentItemsAreRelatedContraLaterally: Code<'static> = c("111155", "Target Content Items are related contra-laterally");
pub const TargetContentItemsAreRelatedSpatially: Code<'static> = c("111154", "Target Content Items are related spatially");
pub const TargetContentItemsAreRelatedTemporally: Code<'static> = c("111153", "Target Content Items are related temporally");
pub const TargetExposureIndex: Code<'static> = c("113846", "Target Exposure Index");
pub const TargetLesionAtBaseline: Code<'static> = c("112074", "Target Lesion at Baseline");
pub const TargetLesionCompleteResponse: Code<'static> = c("112041", "Target Lesion Complete Response");
pub const TargetLesionPartialResponse: Code<'static> = c("112042", "Target Lesion Partial Response");
pub const TargetLesionProgressiveDisease: Code<'static> = c("112043", "Target Lesion Progressive Disease");
pub const TargetLesionStableDisease: Code<'static> = c("112044", "Target Lesion Stable Disease");
pub const TargetNotInTheSpecimen: Code<'static> = c("111445", "Target not in the specimen");
pub const TargetPartiallyObtainedInTheSpecimen: Code<'static> = c("111444", "Target partially obtained in the specimen");
pub const TargetRegion: Code<'static> = c("123014", "Target Region");
pub const Targeted: Code<'static> = c("111127", "Targeted");
pub const Tau: Code<'static> = c("109072", "Tau");
pub const TauM: Code<'static> = c("126330", "tau_m");
pub const TaxonomicRankOfOrigin: Code<'static> = c("127402", "Taxonomic rank of origin");
pub const TeachingFileCategory: Code<'static> = c("128723", "Teaching File Category");
pub const TechNote: Code<'static> = c("121171", "Tech Note");
pub const TechnicalFactorsMissing: Code<'static> = c("111194", "Technical factors missing");
#[deprecated = "retired in PS3.16"]
pub const Technologist: Code<'static> = c("121083", "Technologist");
pub const Teichholz: Code<'static> = c("125209", "Teichholz");
pub const TeletherapyIsotope: Code<'static> = c("130040", "Teletherapy Isotope");
pub const TemperatureEncoded: Code<'static> = c("113067", "Temperature encoded");
pub const TemporalDerivativeExceedsThreshold: Code<'static> = c("126373", "Temporal Derivative Exceeds Threshold");
pub const TemporalDerivativeThreshold: Code<'static> = c("126374", "Temporal Derivative Threshold");
pub const TemporalPredecessor: Code<'static> = c("121348", "Temporal Predecessor");
pub const TemporalSuccessor: Code<'static> = c("121349", "Temporal Successor");
pub const TemporallyRelatedFramesExtractedFromTheSetOfVolumes: Code<'static> = c("113092", "Temporally-related frames extracted from the set of volumes");
pub const TerminatedByScanner: Code<'static> = c("130177", "Terminated by scanner");
pub const TerminatedDueToAirDetected: Code<'static> = c("130176", "Terminated due to air detected");
pub const TerminatedDueToConsumableRemoval: Code<'static> = c("130179", "Terminated due to consumable removal");
pub const TerminatedDueToCriticalBatteryLevel: Code<'static> = c("130178", "Terminated due to critical battery level");
pub const TerminatedDueToExcessiveDurationPause: Code<'static> = c("130158", "Terminated due to excessive duration pause");
pub const TerminatedDueToFlowRateAboveTerminationLimit: Code<'static> = c("130157", "Terminated due to flow rate above termination limit");
pub const TerminatedDueToInjectorCommunicationLoss: Code<'static> = c("130159", "Terminated due to injector communication loss");
pub const TerminatedDueToPressureAboveTerminationLimit: Code<'static> = c("130156", "Terminated due to pressure above termination limit");
pub const TerminatedDueToRequestFromOperator: Code<'static> = c("130154", "Terminated due to request from operator");
pub const TerminatedDueToUnspecifiedInjectorFailure: Code<'static> = c("130160", "Terminated due to unspecified injector failure");
pub const TerminatedHoldDueToTimeout: Code<'static> = c("130269", "Terminated hold due to timeout");
pub const TerritoryRegionSeverity: Code<'static> = c("122459", "Territory Region Severity");
pub const TextureDescriptor: Code<'static> = c("112010", "Texture Descriptor");
pub const TextureMap: Code<'static> = c("TEXTUREMAP", "Texture Map");
pub const ThermalImager: Code<'static> = c("130647", "Thermal Imager");
pub const Thermography: Code<'static> = c("TG", "Thermography");
pub const ThickeningAnalysis: Code<'static> = c("122607", "Thickening Analysis");
pub const ThicknessDeviationCategoryFromNormativeData: Code<'static> = c("111931", "Thickness deviation category from normative data");
pub const ThicknessDeviationFromNormativeData: Code<'static> = c("111932", "Thickness deviation from normative data");
pub const ThinningMyocardium: Code<'static> = c("122114", "Thinning Myocardium");
pub const ThoracicImagingSpecialty: Code<'static> = c("128015", "Thoracic Imaging Specialty");
pub const ThoracicRadiology: Code<'static> = c("128179", "Thoracic Radiology");
pub const ThoracicRespiratoryEffort: Code<'static> = c("130431", "Thoracic Respiratory Effort");
pub const ThoracicRespiratoryInductance: Code<'static> = c("130427", "Thoracic Respiratory Inductance");
pub const ThoracicRespiratoryPVDF: Code<'static> = c("130429", "Thoracic Respiratory PVDF");
pub const ThoraxDiameterSagittal: Code<'static> = c("122221", "Thorax diameter, sagittal");
pub const ThreeDimensionalMethod: Code<'static> = c("112189", "Three-dimensional method");
pub const ThreeVesselAndTracheaView: Code<'static> = c("131026", "Three vessel and trachea view");
pub const ThreeVesselView: Code<'static> = c("131025", "Three vessel view");
pub const ThresholdAttenuationCoefficient: Code<'static> = c("112032", "Threshold Attenuation Coefficient");
pub const ThresholdRelated: Code<'static> = c("111839", "Threshold related");
pub const ThresholdValue: Code<'static> = c("122411", "Threshold Value");
pub const ThroughTransducerIllumination: Code<'static> = c("130813", "Through-transducer illumination");
pub const Time: Code<'static> = c("110858", "Time");
pub const TimeAfterStartOfRadiation: Code<'static> = c("130799", "Time after start of Radiation");
pub const TimeAfterTheStartOfInjection: Code<'static> = c("130194", "Time after the start of injection");
pub const TimeCourseOfSignal: Code<'static> = c("113066", "Time Course of Signal");
pub const TimeDomain: Code<'static> = c("111920", "Time domain");
pub const TimeIntervalSinceDetectionOfContrastBolus: Code<'static> = c("122668", "Time interval since detection of contrast bolus");
pub const TimeIntervalSinceInjectionOfContrastMedia: Code<'static> = c("122665", "Time interval since injection of contrast media");
pub const TimeOfFlight: Code<'static> = c("114201", "Time of flight");
pub const TimeOfFlightAngioMRSignalIntensity: Code<'static> = c("110802", "Time Of Flight Angio MR Signal Intensity");
pub const TimeOfLeadingHalfPeakConcentration: Code<'static> = c("126372", "Time of Leading Half-Peak Concentration");
pub const TimeOfPeakConcentration: Code<'static> = c("126370", "Time of Peak Concentration");
pub const TimePointOrder: Code<'static> = c("126073", "Time Point Order");
pub const TimePointType: Code<'static> = c("126072", "Time Point Type");
pub const TimeRelativeToRWavePeak: Code<'static> = c("122666", "Time relative to R-wave peak");
pub const TimeSinceStartOfStage: Code<'static> = c("122710", "Time since start of stage");
pub const TimeToPeak: Code<'static> = c("113069", "Time To Peak");
pub const TimeToPeakLongitudinalLagrangianStrain: Code<'static> = c("130672", "Time to Peak Longitudinal Lagrangian Strain");
pub const TimeframeUncertain: Code<'static> = c("111399", "Timeframe uncertain");
pub const TimerInterlock: Code<'static> = c("130477", "Timer Interlock");
pub const TissueAirRatio: Code<'static> = c("128433", "Tissue Air Ratio");
pub const TissueFraction: Code<'static> = c("128526", "Tissue Fraction");
pub const TissueMarking: Code<'static> = c("131516", "Tissue marking");
pub const TissueNotOfInterest: Code<'static> = c("130751", "Tissue not of interest");
pub const TissueOfOrigin: Code<'static> = c("127401", "Tissue of origin");
pub const TissueSelectionMethod: Code<'static> = c("112713", "Tissue selection method");
pub const TissueVelocity: Code<'static> = c("110827", "Tissue Velocity");
pub const TitaniumAlloyTitaniumMolybdenum15Zirconium5Aluminium3: Code<'static> = c("130736", "Titanium alloy (Titanium Molybdenum-15 Zirconium-5 Aluminium-3)");
pub const Title21CFRSection900SubpartB: Code<'static> = c("111239", "Title 21 CFR Section 900, Subpart B");
pub const Tmax: Code<'static> = c("113084", "Tmax");
pub const TooSmall: Code<'static> = c("112133", "Too small");
pub const ToolFabrication: Code<'static> = c("129014", "Tool Fabrication");
pub const TopZLocationOfReconstructableVolume: Code<'static> = c("113895", "Top Z Location of Reconstructable Volume");
pub const TopZLocationOfScanningLength: Code<'static> = c("113897", "Top Z Location of Scanning Length");
pub const TopographicBeam: Code<'static> = c("130109", "Topographic Beam");
pub const TotalAcquisitionTime: Code<'static> = c("113855", "Total Acquisition Time");
pub const TotalAntralFollicleCount: Code<'static> = c("130907", "Total Antral Follicle Count");
pub const TotalBodyIrradiation: Code<'static> = c("130341", "Total Body Irradiation");
pub const TotalCholesterolScoreSheetForMen: Code<'static> = c("122235", "Total Cholesterol Score Sheet for Men");
pub const TotalCholesterolScoreSheetForWomen: Code<'static> = c("122236", "Total Cholesterol Score Sheet for Women");
pub const TotalCorneaPowerMeasurementMethod: Code<'static> = c("111758", "Total Cornea Power Measurement Method");
pub const TotalDurationInHousing: Code<'static> = c("127150", "Total duration in housing");
pub const TotalDurationOfLightDarkCycle: Code<'static> = c("127214", "Total duration of light-dark cycle");
pub const TotalFluoroTime: Code<'static> = c("113730", "Total Fluoro Time");
pub const TotalInternalReflectionFluorescence: Code<'static> = c("111749", "Total internal reflection fluorescence");
pub const TotalKeepVeinOpenVolumeAdministered: Code<'static> = c("130165", "Total Keep Vein Open Volume Administered");
pub const TotalLesionGlycolysis: Code<'static> = c("126033", "Total Lesion Glycolysis");
pub const TotalLesionProliferation: Code<'static> = c("126035", "Total Lesion Proliferation");
pub const TotalNumberOfIrradiationEvents: Code<'static> = c("113812", "Total Number of Irradiation Events");
pub const TotalNumberOfManuallyTriggeredInjections: Code<'static> = c("130242", "Total number of manually triggered injections");
pub const TotalNumberOfRadiographicFrames: Code<'static> = c("113731", "Total Number of Radiographic Frames");
pub const TotalPhaseVolumeAdministered: Code<'static> = c("130240", "Total Phase Volume Administered");
pub const TotalPlaqueVolume: Code<'static> = c("122376", "Total Plaque Volume");
pub const TotalPulmonaryResistance: Code<'static> = c("122215", "Total Pulmonary Resistance");
pub const TotalRetinalThicknessILMToBM: Code<'static> = c("111929", "Total retinal thickness (ILM to BM)");
pub const TotalRetinalThicknessILMToISOS: Code<'static> = c("111927", "Total retinal thickness (ILM to IS-OS)");
pub const TotalRetinalThicknessILMToRPE: Code<'static> = c("111928", "Total retinal thickness (ILM to RPE)");
pub const TotalSkinIrradiation: Code<'static> = c("130342", "Total Skin Irradiation");
pub const TotalStepVolumeAdministered: Code<'static> = c("130241", "Total Step Volume Administered");
pub const TotalVascularResistance: Code<'static> = c("122216", "Total Vascular Resistance");
pub const Trace: Code<'static> = c("113201", "Trace");
pub const TracerConcentration: Code<'static> = c("126377", "Tracer Concentration");
pub const TrackingIdentifier: Code<'static> = c("112039", "Tracking Identifier");
pub const TrackingUniqueIdentifier: Code<'static> = c("112040", "Tracking Unique Identifier");
pub const TramlineShadow: Code<'static> = c("112116", "Tramline shadow");
pub const Transcription_110006: Code<'static> = c("110006", "Transcription");
pub const Transcription_110012: Code<'static> = c("110012", "Transcription");
pub const TransesophagealTransducer: Code<'static> = c("125262", "Transesophageal Transducer");
pub const TransformationMatrix: Code<'static> = c("130520", "Transformation Matrix");
pub const TransitTimeTestInjection: Code<'static> = c("130248", "Transit Time Test Injection");
pub const TransmissionIllumination: Code<'static> = c("111741", "Transmission illumination");
pub const TransmitFrequency: Code<'static> = c("122098", "Transmit Frequency");
pub const TransponderMarker: Code<'static> = c("129306", "Transponder Marker");
pub const TransseptalPunctureHeight: Code<'static> = c("131130", "Transseptal puncture height");
pub const TransthalamicCoronalView: Code<'static> = c("131376", "Transthalamic coronal view");
pub const Trastuzumab89Zr: Code<'static> = c("126512", "Trastuzumab ^89^Zr");
pub const TreadmillGradient: Code<'static> = c("122703", "Treadmill gradient");
pub const TreadmillSpeed: Code<'static> = c("122702", "Treadmill speed");
pub const TreatedVolume: Code<'static> = c("130059", "Treated Volume");
pub const TreatmentDeliveryType: Code<'static> = c("121740", "Treatment Delivery Type");
pub const TreatmentExceedsPatientTolerance: Code<'static> = c("130665", "Treatment exceeds patient tolerance");
pub const TreatmentMachineIsocenter: Code<'static> = c("130359", "Treatment Machine Isocenter");
pub const TreatmentPlanningNote: Code<'static> = c("130024", "Treatment Planning Note");
pub const TreatmentTerminated: Code<'static> = c("130453", "Treatment Terminated");
pub const TreatmentUnnecessary: Code<'static> = c("130664", "Treatment unnecessary");
pub const TreeInBudSign: Code<'static> = c("112127", "Tree-in-bud sign");
pub const Trend: Code<'static> = c("122151", "Trend");
pub const TricuspidAnnulusArea: Code<'static> = c("131208", "Tricuspid annulus area");
pub const TricuspidAnnulusAreaDiastolicSystolicRatio: Code<'static> = c("131209", "Tricuspid annulus area diastolic systolic ratio");
pub const TricuspidAnnulusPerimeter: Code<'static> = c("131210", "Tricuspid annulus perimeter");
pub const TricuspidValveAnnulusDiameter: Code<'static> = c("131061", "Tricuspid valve annulus diameter");
pub const TricuspidValveCoaptationLength: Code<'static> = c("131211", "Tricuspid valve coaptation length");
pub const TricuspidValveMajorAxisDiastole: Code<'static> = c("131212", "Tricuspid valve major axis diastole");
pub const TricuspidValveMinorAxis: Code<'static> = c("131213", "Tricuspid valve minor axis");
pub const TricuspidValveSphericityIndex: Code<'static> = c("131214", "Tricuspid valve sphericity index");
pub const TricuspidValveTentingHeight: Code<'static> = c("131215", "Tricuspid valve tenting height");
pub const TricuspidValveTentingVolume: Code<'static> = c("131216", "Tricuspid valve tenting volume");
pub const TrigeminalNodeSet: Code<'static> = c("130364", "Trigeminal Node Set");
pub const TrueLumen: Code<'static> = c("122360", "True Lumen");
pub const TruncatedGaussianDiffusionModel: Code<'static> = c("113255", "Truncated Gaussian diffusion model");
pub const TrustRegion: Code<'static> = c("113266", "Trust-Region");
pub const TubularShadow: Code<'static> = c("112117", "Tubular shadow");
pub const TumorBoardInputUsed: Code<'static> = c("128221", "Tumor Board Input Used");
pub const TumorGraft: Code<'static> = c("127460", "Tumor graft");
pub const TumorRegistryInputUsed: Code<'static> = c("128222", "Tumor Registry Input Used");
pub const TurbulenceResistance: Code<'static> = c("122550", "Turbulence Resistance");
pub const TurntableScanMethod: Code<'static> = c("114209", "Turntable Scan Method");
pub const TwoChamber: Code<'static> = c("122565", "Two Chamber");
pub const TwoCompartmentExchangeModel: Code<'static> = c("126347", "Two Compartment Exchange Model");
pub const TwoDimensionalMethod: Code<'static> = c("112188", "Two-dimensional method");
pub const TypeOfContent: Code<'static> = c("112009", "Type of Content");
pub const TypeOfMyocardialInfarction: Code<'static> = c("122170", "Type of Myocardial Infarction");
pub const UDI: Code<'static> = c("128739", "UDI");
pub const URI: Code<'static> = c("110037", "URI");
pub const US3DCAMModel: Code<'static> = c("129018", "US 3D CAM model");
pub const USBDiskEmulation: Code<'static> = c("110030", "USB Disk Emulation");
pub const Ublituximab89Zr: Code<'static> = c("126739", "Ublituximab ^89^Zr");
pub const Ultrasound: Code<'static> = c("US", "Ultrasound");
pub const UltrasoundAttenuation: Code<'static> = c("110833", "Ultrasound Attenuation");
pub const UltrasoundAttenuationCoefficient: Code<'static> = c("131341", "Ultrasound Attenuation Coefficient");
pub const UltrasoundAttenuationImaging: Code<'static> = c("131340", "Ultrasound Attenuation Imaging");
pub const UltrasoundBoneDensitometry: Code<'static> = c("BDUS", "Ultrasound Bone Densitometry");
pub const UltrasoundContact: Code<'static> = c("111750", "Ultrasound Contact");
pub const UltrasoundImagingSubjectMatter: Code<'static> = c("128736", "Ultrasound imaging subject matter");
pub const UltrasoundImmersion: Code<'static> = c("111751", "Ultrasound Immersion");
#[deprecated = "retired in PS3.16"]
pub const Ultrasound_111137: Code<'static> = c("111137", "Ultrasound");
pub const UmbilicocerebralRatio: Code<'static> = c("131010", "Umbilicocerebral ratio");
pub const UnavailabilityOfABeamModifier: Code<'static> = c("130466", "Unavailability of a Beam Modifier");
pub const Unavailable: Code<'static> = c("UNAVAILABLE", "Unavailable");
pub const UnclassifiedCombination: Code<'static> = c("130081", "Unclassified Combination");
pub const UnclassifiedVolume: Code<'static> = c("130048", "Unclassified Volume");
pub const UncompressedPredecessor: Code<'static> = c("121320", "Uncompressed predecessor");
pub const UnderExposed: Code<'static> = c("111211", "Under exposed");
pub const Underflow: Code<'static> = c("114004", "Underflow");
pub const UndeterminedSex: Code<'static> = c("121103", "Undetermined sex");
pub const UndilutedDoseAdministered: Code<'static> = c("122092", "Undiluted dose administered");
pub const Unheated: Code<'static> = c("127255", "Unheated");
pub const UniformSpeedOfSoundCorrection: Code<'static> = c("130818", "Uniform Speed of Sound Correction");
pub const Uninvolved: Code<'static> = c("111470", "Uninvolved");
pub const UniqueDeviceIdentifiers: Code<'static> = c("121000", "Unique Device Identifiers");
pub const UnitSerialIdentifier: Code<'static> = c("121148", "Unit Serial Identifier");
pub const UnitedAllBreedRegistry: Code<'static> = c("109217", "United All Breed Registry");
pub const UnitedKennelClub: Code<'static> = c("109218", "United Kennel Club");
pub const UniversalKennelClubInternational: Code<'static> = c("109219", "Universal Kennel Club International");
pub const UnknownFailure: Code<'static> = c("111221", "Unknown failure");
pub const UnknownSex: Code<'static> = c("U", "Unknown Sex");
pub const UnmountedDetector: Code<'static> = c("113953", "Unmounted Detector");
pub const UnscannedTissue: Code<'static> = c("131517", "Unscanned tissue");
pub const UnsharpMask: Code<'static> = c("113089", "Unsharp mask");
pub const Unspecified: Code<'static> = c("111176", "Unspecified");
pub const UnspecifiedGynecologicalHormone: Code<'static> = c("111542", "Unspecified gynecological hormone");
pub const UnspecifiedMethodOfCalculation: Code<'static> = c("112187", "Unspecified method of calculation");
pub const UntrackableMeasurement: Code<'static> = c("125304", "Untrackable Measurement");
pub const UnusableQualityRendersImageUnusable: Code<'static> = c("111235", "Unusable - Quality renders image unusable");
pub const UnusableTissue: Code<'static> = c("131502", "Unusable tissue");
pub const Unused: Code<'static> = c("127177", "Unused");
pub const UnwantedSubtractionImages: Code<'static> = c("130582", "Unwanted subtraction images");
pub const UprightStandMount: Code<'static> = c("113954", "Upright Stand Mount");
pub const UsableDoesNotMeetTheQualityControlStandard: Code<'static> = c("111236", "Usable - Does not meet the quality control standard");
pub const UsableMeetsTheQualityControlStandard: Code<'static> = c("111237", "Usable - Meets the quality control standard");
pub const UsableTissue: Code<'static> = c("131501", "Usable tissue");
pub const UseOfRestrictedFunction: Code<'static> = c("110132", "Use of Restricted Function");
pub const UsedSubstanceType: Code<'static> = c("111546", "Used Substance Type");
pub const UserAuthentication: Code<'static> = c("110114", "User Authentication");
pub const UserChosenValue: Code<'static> = c("121410", "User chosen value");
pub const UserDefinedAIFROI: Code<'static> = c("126362", "User-defined AIF ROI");
pub const UserSecurityAttributesChanged: Code<'static> = c("110137", "User security Attributes Changed");
pub const UserSelectedFiducial: Code<'static> = c("112356", "User Selected Fiducial");
pub const UserSelectedMethod: Code<'static> = c("122575", "User selected method");
pub const UserSelectedVolumeFlow: Code<'static> = c("128314", "User selected volume flow");
pub const UserSelectedVolumeStructureMap: Code<'static> = c("128315", "User selected volume structure map");
pub const UterineMalformations: Code<'static> = c("111565", "Uterine malformations");
pub const UtricleCyst: Code<'static> = c("130569", "Utricle cyst");
pub const VMAT: Code<'static> = c("130107", "VMAT");
pub const VMaxMyocardial: Code<'static> = c("109073", "V max myocardial");
pub const VO2Equals152BSA: Code<'static> = c("122250", "VO2 = 152 * BSA");
pub const VO2Equals175BSA: Code<'static> = c("122251", "VO2 = 175 * BSA");
pub const VO2Equals176BSA: Code<'static> = c("122252", "VO2 = 176 * BSA");
pub const VO2EqualsVeSTPD10FIO2FE02: Code<'static> = c("122249", "VO2 = VeSTPD * 10 * (FIO2 - FE02)");
pub const VO2femaleEqualsBSA13811704LogeAgePlus0378HRf: Code<'static> = c("122248", "VO2female = BSA (138.1 - 17.04 * loge(age) + 0.378 * HRf)");
pub const VO2maleEqualsBSA13811149LogeAgePlus0378HRf: Code<'static> = c("122247", "VO2male = BSA (138.1 - 11.49 * loge(age) + 0.378 * HRf)");
pub const VVulnerablePlaque: Code<'static> = c("130735", "V - Vulnerable plaque");
pub const VWave: Code<'static> = c("109052", "V wave");
pub const VWaveOfNextBeat: Code<'static> = c("109053", "V wave of next beat");
pub const VWavePeakPressure: Code<'static> = c("109034", "V wave peak pressure");
pub const VWavePressureAverage: Code<'static> = c("109035", "V wave pressure, average");
pub const VacuumAssisted: Code<'static> = c("111490", "Vacuum assisted");
pub const VacuumMold: Code<'static> = c("130118", "Vacuum Mold");
pub const VaginalCylinder: Code<'static> = c("130121", "Vaginal Cylinder");
pub const ValueBasedImage: Code<'static> = c("129322", "Value-based Image");
pub const ValueIndeterminate: Code<'static> = c("114011", "Value indeterminate");
pub const ValueOutOfRange: Code<'static> = c("114009", "Value out of range");
pub const ValueTiming: Code<'static> = c("130518", "Value Timing");
pub const ValueUnknown: Code<'static> = c("114010", "Value unknown");
pub const ValveClose: Code<'static> = c("109036", "Valve close");
pub const ValveOpen: Code<'static> = c("109037", "Valve open");
pub const ValvularIntervention: Code<'static> = c("122053", "Valvular Intervention");
pub const VariableCircularCollimator: Code<'static> = c("130332", "Variable Circular Collimator");
pub const VascularAndInterventionalImagingSubjectMatter: Code<'static> = c("128734", "Vascular and interventional imaging subject matter");
pub const VascularFunctionalAnalysis: Code<'static> = c("122606", "Vascular Functional Analysis");
pub const VascularImagingSpecialty: Code<'static> = c("128017", "Vascular Imaging Specialty");
pub const VascularIntervention: Code<'static> = c("122056", "Vascular Intervention");
pub const VascularMorphologicalAnalysis: Code<'static> = c("122605", "Vascular Morphological Analysis");
#[deprecated = "retired in PS3.16"]
pub const VascularUltrasoundProcedureReport: Code<'static> = c("125100", "Vascular Ultrasound Procedure Report");
pub const VascularVolumeMeasurementLength: Code<'static> = c("122336", "Vascular Volume measurement length");
pub const Vascularity: Code<'static> = c("111372", "Vascularity");
pub const VascularityNotAssessed: Code<'static> = c("111374", "Vascularity not assessed");
pub const VascularityNotPresent: Code<'static> = c("111373", "Vascularity not present");
pub const VascularityPresentImmediatelyAdjacentToLesion: Code<'static> = c("111376", "Vascularity present immediately adjacent to lesion");
pub const VascularityPresentInLesion: Code<'static> = c("111375", "Vascularity present in lesion");
pub const Vasoconstriction: Code<'static> = c("112077", "Vasoconstriction");
pub const Vasodilation: Code<'static> = c("112078", "Vasodilation");
pub const Ve: Code<'static> = c("126314", "ve");
pub const Vector: Code<'static> = c("121225", "Vector");
pub const VeilingGlareEvaluation: Code<'static> = c("109707", "Veiling glare evaluation");
pub const VelocityEncoded: Code<'static> = c("113070", "Velocity encoded");
pub const VelocityEncodingMaximumValue: Code<'static> = c("122643", "Velocity Encoding Maximum Value");
pub const VelocityEncodingMinimumValue: Code<'static> = c("122642", "Velocity Encoding Minimum Value");
pub const VenaContractaWidth: Code<'static> = c("125334", "Vena Contracta Width");
pub const VenousDopplerScore: Code<'static> = c("131034", "Venous Doppler Score");
pub const VentralDiencephalon: Code<'static> = c("110700", "Ventral Diencephalon");
pub const VentricularAnalysis: Code<'static> = c("122601", "Ventricular Analysis");
pub const VentricularEndDiastolicPressure: Code<'static> = c("122191", "Ventricular End Diastolic pressure");
pub const VentricularPressureMeasurements: Code<'static> = c("122122", "Ventricular pressure measurements");
pub const VentricularSystolicBloodPressure: Code<'static> = c("122194", "Ventricular Systolic blood pressure");
pub const VentriculoatrialTimeInterval: Code<'static> = c("131001", "Ventriculoatrial time interval");
pub const Verbal: Code<'static> = c("121004", "Verbal");
pub const Verifying: Code<'static> = c("121098", "Verifying");
pub const VertebralIntervertebralNotch: Code<'static> = c("112097", "Vertebral Intervertebral Notch");
pub const VerticalPixelSpacing: Code<'static> = c("111066", "Vertical Pixel Spacing");
pub const VerticalPrismBase: Code<'static> = c("111678", "Vertical Prism Base");
pub const VerticalPrismPower: Code<'static> = c("111677", "Vertical Prism Power");
pub const VerySmall: Code<'static> = c("112132", "Very small");
pub const VeryStrongFamilyHistoryOfBreastCancer: Code<'static> = c("111561", "Very strong family history of breast cancer");
pub const VesselBranch: Code<'static> = c("125101", "Vessel Branch");
pub const VesselLumenCrossSectionalArea: Code<'static> = c("122480", "Vessel Lumen Cross-Sectional Area");
pub const VesselMorphology: Code<'static> = c("122134", "Vessel Morphology");
pub const VideoRecordingOff: Code<'static> = c("130888", "Video recording off");
pub const VideoRecordingOn: Code<'static> = c("130887", "Video recording on");
pub const VideoTapeDigitized: Code<'static> = c("110022", "Video Tape Digitized");
pub const VideoTapeDigitizerEquipment: Code<'static> = c("VIDD", "Video Tape Digitizer Equipment");
#[deprecated = "retired in PS3.16"]
pub const Videofluorography: Code<'static> = c("VF", "Videofluorography");
pub const ViewAndLateralityMarkerDoesNotHaveApprovedCodes: Code<'static> = c("111179", "View and Laterality Marker does not have approved codes");
pub const ViewAndLateralityMarkerDoesNotHaveBothViewAndLaterality: Code<'static> = c("111178", "View and Laterality Marker does not have both view and laterality");
pub const ViewAndLateralityMarkerIsIncorrect: Code<'static> = c("111183", "View and Laterality Marker is incorrect");
pub const ViewAndLateralityMarkerIsMissing: Code<'static> = c("111177", "View and Laterality Marker is missing");
pub const ViewAndLateralityMarkerIsNotNearTheAxilla: Code<'static> = c("111180", "View and Laterality Marker is not near the axilla");
pub const ViewAndLateralityMarkerIsOffImage: Code<'static> = c("111184", "View and Laterality Marker is off image");
pub const ViewAndLateralityMarkerIsPartiallyObscured: Code<'static> = c("111182", "View and Laterality Marker is partially obscured");
pub const ViewAndLateralityMarkerOverlapsBreastTissue: Code<'static> = c("111181", "View and Laterality Marker overlaps breast tissue");
pub const Vigneswaran2018: Code<'static> = c("131532", "Vigneswaran 2018");
pub const VirginalHyperplasia: Code<'static> = c("111298", "Virginal hyperplasia");
pub const VirtualGrid: Code<'static> = c("111640", "Virtual grid");
pub const ViscosityAt37C: Code<'static> = c("130186", "Viscosity at 37C");
pub const VisibleReflectorMarker: Code<'static> = c("129310", "Visible Reflector Marker");
pub const VisualAcuity: Code<'static> = c("VA", "Visual Acuity");
pub const VisualAlignment: Code<'static> = c("125025", "Visual Alignment");
pub const VisualExplanation: Code<'static> = c("130401", "Visual explanation");
pub const VisualField07TestPattern: Code<'static> = c("111812", "Visual Field 07 Test Pattern");
pub const VisualField102TestPattern: Code<'static> = c("111801", "Visual Field 10-2 Test Pattern");
pub const VisualField1LTTestStrategy: Code<'static> = c("111828", "Visual Field 1-LT Test Strategy");
pub const VisualField242TestPattern: Code<'static> = c("111800", "Visual Field 24-2 Test Pattern");
pub const VisualField2LTDynamicTestStrategy: Code<'static> = c("111833", "Visual Field 2LT-Dynamic Test Strategy");
pub const VisualField2LTNormalTestStrategy: Code<'static> = c("111834", "Visual Field 2LT-Normal Test Strategy");
pub const VisualField2LTTestStrategy: Code<'static> = c("111829", "Visual Field 2-LT Test Strategy");
pub const VisualField302TestPattern: Code<'static> = c("111802", "Visual Field 30-2 Test Pattern");
pub const VisualField604TestPattern: Code<'static> = c("111803", "Visual Field 60-4 Test Pattern");
pub const VisualFieldCLASSStrategy: Code<'static> = c("111837", "Visual Field CLASS Strategy");
pub const VisualFieldCLIPTestStrategy: Code<'static> = c("111836", "Visual Field CLIP Test Strategy");
pub const VisualFieldCentral40PointTestPattern: Code<'static> = c("111805", "Visual Field Central 40 Point Test Pattern");
pub const VisualFieldCentral76PointTestPattern: Code<'static> = c("111806", "Visual Field Central 76 Point Test Pattern");
pub const VisualFieldCentralTestPattern: Code<'static> = c("111814", "Visual Field Central Test Pattern");
pub const VisualFieldDynamicTestStrategy: Code<'static> = c("111826", "Visual Field Dynamic Test Strategy");
pub const VisualFieldFastPacTestStrategy: Code<'static> = c("111819", "Visual Field FastPac Test Strategy");
pub const VisualFieldFastThresholdTestStrategy: Code<'static> = c("111835", "Visual Field Fast Threshold Test Strategy");
pub const VisualFieldFullField120PointTestPattern: Code<'static> = c("111809", "Visual Field Full Field 120 Point Test Pattern");
pub const VisualFieldFullField81PointTestPattern: Code<'static> = c("111808", "Visual Field Full Field 81 Point Test Pattern");
pub const VisualFieldFullFromPriorTestStrategy: Code<'static> = c("111820", "Visual Field Full From Prior Test Strategy");
pub const VisualFieldFullThresholdTestStrategy: Code<'static> = c("111818", "Visual Field Full Threshold Test Strategy");
pub const VisualFieldGATETestStrategy: Code<'static> = c("111831", "Visual Field GATE Test Strategy");
pub const VisualFieldGATEiTestStrategy: Code<'static> = c("111832", "Visual Field GATEi Test Strategy");
pub const VisualFieldGTestPattern: Code<'static> = c("111810", "Visual Field G Test Pattern");
pub const VisualFieldGlobalDeviationFromNormal: Code<'static> = c("131248", "Visual Field Global Deviation from Normal");
pub const VisualFieldIndex: Code<'static> = c("111852", "Visual Field Index");
pub const VisualFieldKeyMeasurements: Code<'static> = c("131240", "Visual Field Key Measurements");
pub const VisualFieldLVCTestPattern: Code<'static> = c("111813", "Visual Field LVC Test Pattern");
pub const VisualFieldLVSTestStrategy: Code<'static> = c("111830", "Visual Field LVS Test Strategy");
pub const VisualFieldLocalizedDeviationFromNormal: Code<'static> = c("131249", "Visual Field Localized Deviation From Normal");
pub const VisualFieldLossDueToDiffuseDefect: Code<'static> = c("111853", "Visual Field Loss Due to Diffuse Defect");
pub const VisualFieldLossDueToLocalDefect: Code<'static> = c("111854", "Visual Field Loss Due to Local Defect");
pub const VisualFieldMTestPattern: Code<'static> = c("111811", "Visual Field M Test Pattern");
pub const VisualFieldMaculaTestPattern: Code<'static> = c("111804", "Visual Field Macula Test Pattern");
pub const VisualFieldNormalTestStrategy: Code<'static> = c("111827", "Visual Field Normal Test Strategy");
pub const VisualFieldOptimaTestStrategy: Code<'static> = c("111821", "Visual Field Optima Test Strategy");
pub const VisualFieldPeripheral60PointTestPattern: Code<'static> = c("111807", "Visual Field Peripheral 60 Point Test Pattern");
pub const VisualFieldQuantifyDefectsTestStrategy: Code<'static> = c("111824", "Visual Field Quantify-Defects Test Strategy");
pub const VisualFieldSITAFastTestStrategy: Code<'static> = c("111817", "Visual Field SITA-Fast Test Strategy");
pub const VisualFieldSITASWAPTestStrategy: Code<'static> = c("111816", "Visual Field SITA-SWAP Test Strategy");
pub const VisualFieldSITAStandardTestStrategy: Code<'static> = c("111815", "Visual Field SITA-Standard Test Strategy");
pub const VisualFieldTOPTestStrategy: Code<'static> = c("111825", "Visual Field TOP Test Strategy");
pub const VisualFieldThreeZoneTestStrategy: Code<'static> = c("111823", "Visual Field Three-Zone Test Strategy");
pub const VisualFieldTwoZoneTestStrategy: Code<'static> = c("111822", "Visual Field Two-Zone Test Strategy");
pub const VisualFixationQualityDuringAcquisition: Code<'static> = c("111696", "Visual Fixation Quality During Acquisition");
pub const VisualFixationQualityProblem: Code<'static> = c("111697", "Visual Fixation Quality Problem");
pub const VisualMarkingOfSpecimen: Code<'static> = c("111723", "Visual Marking of Specimen");
pub const VitreousStructuralReflectanceMap: Code<'static> = c("128262", "Vitreous structural reflectance map");
pub const VitreousVasculatureFlow: Code<'static> = c("128261", "Vitreous vasculature flow");
pub const Voice: Code<'static> = c("109110", "Voice");
pub const VoltageMeasurement: Code<'static> = c("109013", "Voltage measurement");
pub const VoltageMeasurementByBasketCatheter: Code<'static> = c("109011", "Voltage measurement by basket catheter");
pub const VoltageMeasurementByMappingCatheter: Code<'static> = c("109012", "Voltage measurement by mapping catheter");
pub const VolumeAdministered: Code<'static> = c("122091", "Volume administered");
pub const VolumeBasedMethod: Code<'static> = c("122652", "Volume Based Method");
pub const VolumeCorrespondingToSpatiallyRelatedAcquisitionFrames: Code<'static> = c("121347", "Volume corresponding to spatially-related acquisition frames");
pub const VolumeEditing: Code<'static> = c("113077", "Volume editing");
pub const VolumeEstimatedFromSingle2DRegion: Code<'static> = c("121216", "Volume estimated from single 2D region");
pub const VolumeEstimatedFromThreeOrMoreNonCoplanar2DRegions: Code<'static> = c("121217", "Volume estimated from three or more non-coplanar 2D regions");
pub const VolumeEstimatedFromTwoNonCoplanar2DRegions: Code<'static> = c("121218", "Volume estimated from two non-coplanar 2D regions");
pub const VolumeMethod: Code<'static> = c("122429", "Volume Method");
pub const VolumeOccupancyImage: Code<'static> = c("129328", "Volume Occupancy Image");
pub const VolumeOfBoundingThreeDimensionalRegion: Code<'static> = c("121219", "Volume of bounding three dimensional region");
pub const VolumeOfBreast: Code<'static> = c("112193", "Volume of breast");
pub const VolumeOfCircumscribedSphere: Code<'static> = c("121220", "Volume of circumscribed sphere");
pub const VolumeOfEllipsoid: Code<'static> = c("121221", "Volume of ellipsoid");
pub const VolumeOfParenchymalTissue: Code<'static> = c("112192", "Volume of parenchymal tissue");
pub const VolumeOfSphere: Code<'static> = c("121222", "Volume of sphere");
pub const VolumeOfUse: Code<'static> = c("111580", "Volume of use");
pub const VolumeOfVascularCalcification: Code<'static> = c("112197", "Volume of Vascular Calcification");
pub const VolumeRangeMaximumValue: Code<'static> = c("130905", "Volume Range Maximum Value");
pub const VolumeRangeMinimumValue: Code<'static> = c("130904", "Volume Range Minimum Value");
pub const VolumeRatio: Code<'static> = c("113288", "Volume Ratio");
pub const VolumeRendering: Code<'static> = c("113074", "Volume rendering");
pub const VolumeScanPattern: Code<'static> = c("125242", "Volume scan pattern");
pub const VolumeScoringMethod: Code<'static> = c("112056", "Volume scoring method");
pub const VolumeSurface: Code<'static> = c("121231", "Volume Surface");
pub const VolumeUnadministeredOrDiscarded: Code<'static> = c("122096", "Volume unadministered or discarded");
pub const VolumetricDiffusionDxxComponent: Code<'static> = c("110810", "Volumetric Diffusion Dxx Component");
pub const VolumetricDiffusionDxyComponent: Code<'static> = c("110811", "Volumetric Diffusion Dxy Component");
pub const VolumetricDiffusionDxzComponent: Code<'static> = c("110812", "Volumetric Diffusion Dxz Component");
pub const VolumetricDiffusionDyyComponent: Code<'static> = c("110813", "Volumetric Diffusion Dyy Component");
pub const VolumetricDiffusionDyzComponent: Code<'static> = c("110814", "Volumetric Diffusion Dyz Component");
pub const VolumetricDiffusionDzzComponent: Code<'static> = c("110815", "Volumetric Diffusion Dzz Component");
pub const VoxelizedRadiationTransportModel: Code<'static> = c("128422", "Voxelized Radiation Transport Model");
pub const VoxelwiseSelectionOfBValue: Code<'static> = c("113285", "Voxelwise selection of b-value");
pub const Vp: Code<'static> = c("126331", "vp");
pub const VulnerablePlaque: Code<'static> = c("122389", "Vulnerable Plaque");
pub const WHO: Code<'static> = c("112029", "WHO");
pub const WSI20XRGB: Code<'static> = c("112704", "WSI 20X RGB");
pub const WSI40XRGB: Code<'static> = c("112705", "WSI 40X RGB");
pub const WallMass: Code<'static> = c("122447", "Wall Mass");
pub const WallStress: Code<'static> = c("122448", "Wall Stress");
pub const WallThickness: Code<'static> = c("122445", "Wall Thickness");
pub const WallThicknessRatioEndSystolicToEndDiastolic: Code<'static> = c("122624", "Wall Thickness Ratio end-systolic to end-diastolic");
pub const WallToSeptum: Code<'static> = c("110877", "Wall To Septum");
pub const WallVolume: Code<'static> = c("122446", "Wall Volume");
pub const WashoutDelayedPhaseType3Curve: Code<'static> = c("130599", "Washout delayed phase: Type 3 curve");
pub const WaterEquivalentDiameter: Code<'static> = c("113980", "Water Equivalent Diameter");
pub const WaterEquivalentDiameterAtLongitudinalPositionZ: Code<'static> = c("113995", "Water Equivalent Diameter At Longitudinal Position Z");
pub const WaterEquivalentDiameterFromLimitedFOVImages: Code<'static> = c("113992", "Water Equivalent Diameter From Limited FOV Images");
pub const WaterEquivalentDiameterFromLocalizer: Code<'static> = c("113984", "Water Equivalent Diameter From Localizer");
pub const WaterEquivalentDiameterFromRawData: Code<'static> = c("113983", "Water Equivalent Diameter From Raw Data");
pub const WaterEquivalentDiameterIntegratedAcrossScanRange: Code<'static> = c("113982", "Water Equivalent Diameter Integrated Across Scan Range");
pub const WaterEquivalentDiameterRepresentativeValue: Code<'static> = c("113981", "Water Equivalent Diameter Representative Value");
pub const WaterFraction: Code<'static> = c("129103", "Water fraction");
pub const WaterPerFatInPhase: Code<'static> = c("129101", "Water/fat in phase");
pub const WaterPerFatOutOfPhase: Code<'static> = c("129102", "Water/fat out of phase");
pub const WaveformAcquired: Code<'static> = c("121143", "Waveform Acquired");
pub const WaveformAnnotationGroup: Code<'static> = c("130872", "Waveform Annotation Group");
pub const WaveformAnnotationGroupLabel: Code<'static> = c("130874", "Waveform Annotation Group Label");
pub const WaveformAnnotationGroupNumber: Code<'static> = c("130873", "Waveform Annotation Group Number");
pub const WaveformAnnotationModifier: Code<'static> = c("130875", "Waveform Annotation Modifier");
pub const WaveformAnnotations: Code<'static> = c("130870", "Waveform Annotations");
pub const WaveformLibrary: Code<'static> = c("130877", "Waveform Library");
pub const WaveformLibraryEntryMultiplexGroupDescriptors: Code<'static> = c("130879", "Waveform Library Entry Multiplex Group Descriptors");
pub const WaveformLibraryGroup: Code<'static> = c("130878", "Waveform Library Group");
pub const WeakFamilyHistoryOfBreastCancer: Code<'static> = c("111559", "Weak family history of breast cancer");
pub const WedgeFilter: Code<'static> = c("113651", "Wedge filter");
pub const Wedging: Code<'static> = c("112321", "Wedging");
pub const WellDemarcated: Code<'static> = c("112139", "Well demarcated");
pub const WhiteMatterT1Hypointensity: Code<'static> = c("110701", "White Matter T1 Hypointensity");
pub const WhiteMatterT2Hyperintensity: Code<'static> = c("110702", "White Matter T2 Hyperintensity");
pub const WhitePattern: Code<'static> = c("109941", "White Pattern");
pub const WholeBodyPod: Code<'static> = c("130119", "Whole Body Pod");
pub const WholeEyeStructuralReflectanceMap: Code<'static> = c("128278", "Whole eye structural reflectance map");
pub const WholeEyeVasculatureFlow: Code<'static> = c("128277", "Whole eye vasculature flow");
pub const WholeSlideImaging: Code<'static> = c("112703", "Whole Slide Imaging");
pub const WidthDescriptor: Code<'static> = c("112026", "Width Descriptor");
pub const WinstonLutzPointer: Code<'static> = c("130353", "Winston-Lutz Pointer");
pub const WireMarker: Code<'static> = c("129305", "Wire Marker");
pub const WorkingCanineAssociationOfCanada: Code<'static> = c("109220", "Working Canine Association of Canada");
pub const WorkitemAssignmentRejectedByAssignedResource: Code<'static> = c("110530", "Workitem assignment rejected by assigned resource");
pub const WorkitemExpired: Code<'static> = c("110533", "Workitem expired");
pub const Workstation: Code<'static> = c("WSD", "Workstation");
pub const WorldKennelClub: Code<'static> = c("109221", "World Kennel Club");
pub const WorldWideKennelClub: Code<'static> = c("109222", "World Wide Kennel Club");
pub const XConcept: Code<'static> = c("122698", "X-Concept");
pub const XDescentPressure: Code<'static> = c("122208", "x-descent pressure");
pub const XOrientation: Code<'static> = c("130334", "X Orientation");
pub const XRayAngiography: Code<'static> = c("XA", "X-Ray Angiography");
pub const XRayAttenuation: Code<'static> = c("110850", "X-Ray Attenuation");
#[deprecated = "retired in PS3.16"]
pub const XRayAttenuationCoefficient: Code<'static> = c("110851", "X-Ray Attenuation Coefficient");
pub const XRayAttenuatorModelData: Code<'static> = c("128470", "X-Ray Attenuator Model Data");
pub const XRayBeamAttenuator: Code<'static> = c("128457", "X-Ray Beam Attenuator");
pub const XRayBeamAttenuatorModel: Code<'static> = c("128472", "X-Ray Beam Attenuator Model");
pub const XRayBeamAttenuatorModelReference: Code<'static> = c("128474", "X-Ray Beam Attenuator Model Reference");
pub const XRayBeamAttenuatorModelRegistration: Code<'static> = c("128475", "X-Ray Beam Attenuator Model Registration");
pub const XRayDetectorDataAvailable: Code<'static> = c("113945", "X-Ray Detector Data Available");
pub const XRayFilterAluminumEquivalent: Code<'static> = c("113821", "X-Ray Filter Aluminum Equivalent");
pub const XRayFilterMaterial: Code<'static> = c("113757", "X-Ray Filter Material");
pub const XRayFilterThickness: Code<'static> = c("130509", "X-Ray Filter Thickness");
pub const XRayFilterThicknessMaximum: Code<'static> = c("113773", "X-Ray Filter Thickness Maximum");
pub const XRayFilterThicknessMinimum: Code<'static> = c("113758", "X-Ray Filter Thickness Minimum");
pub const XRayFilterType: Code<'static> = c("113772", "X-Ray Filter Type");
pub const XRayFilters: Code<'static> = c("113771", "X-Ray Filters");
pub const XRayFluoroscopyOnSignal: Code<'static> = c("109004", "X-Ray Fluoroscopy On Signal");
pub const XRayGrid: Code<'static> = c("111635", "X-Ray Grid");
pub const XRayMechanicalDataAvailable: Code<'static> = c("113944", "X-Ray Mechanical Data Available");
pub const XRayModulationType: Code<'static> = c("113842", "X-Ray Modulation Type");
pub const XRayOnTrigger: Code<'static> = c("109005", "X-Ray On Trigger");
pub const XRayRadiationDoseReport: Code<'static> = c("113701", "X-Ray Radiation Dose Report");
pub const XRayReadingDevice: Code<'static> = c("113942", "X-Ray Reading Device");
pub const XRaySourceDataAvailable: Code<'static> = c("113943", "X-Ray Source Data Available");
pub const XRaySourceReferenceCoordinateSystem: Code<'static> = c("130519", "X-Ray Source Reference Coordinate System");
pub const XRayTomographyImageReconstruction: Code<'static> = c("110018", "X-Ray Tomography Image Reconstruction");
pub const XRayTubeCurrent: Code<'static> = c("113734", "X-Ray Tube Current");
pub const XmAb557489Zr: Code<'static> = c("126734", "XmAb5574 ^89^Zr");
pub const YBRFULLCBComponent: Code<'static> = c("110838", "YBR FULL CB Component");
pub const YBRFULLCRComponent: Code<'static> = c("110839", "YBR FULL CR Component");
pub const YBRFULLYComponent: Code<'static> = c("110837", "YBR FULL Y Component");
pub const YBRICTCBComponent: Code<'static> = c("110844", "YBR ICT CB Component");
pub const YBRICTCRComponent: Code<'static> = c("110845", "YBR ICT CR Component");
pub const YBRICTYComponent: Code<'static> = c("110843", "YBR ICT Y Component");
pub const YBRPARTIALCBComponent: Code<'static> = c("110841", "YBR PARTIAL CB Component");
pub const YBRPARTIALCRComponent: Code<'static> = c("110842", "YBR PARTIAL CR Component");
pub const YBRPARTIALYComponent: Code<'static> = c("110840", "YBR PARTIAL Y Component");
pub const YBRRCTCBComponent: Code<'static> = c("110847", "YBR RCT CB Component");
pub const YBRRCTCRComponent: Code<'static> = c("110848", "YBR RCT CR Component");
pub const YBRRCTYComponent: Code<'static> = c("110846", "YBR RCT Y Component");
pub const YConcept: Code<'static> = c("122699", "Y-Concept");
pub const YDescentPressure: Code<'static> = c("122209", "y-descent pressure");
pub const YOrientation: Code<'static> = c("130335", "Y Orientation");
#[deprecated = "retired in PS3.16"]
pub const YellowGreenFilter: Code<'static> = c("111604", "Yellow-green filter");
pub const ZPointPressure: Code<'static> = c("122210", "z-point pressure");
pub const ZScore: Code<'static> = c("113071", "Z-Score");
pub const ZScoreOfMeasurement: Code<'static> = c("121416", "Z-Score of measurement");
pub const ZValueOfLocationOfWaterEquivalentDiameterEstimation: Code<'static> = c("113986", "Z value of location of Water Equivalent Diameter estimation");
pub const Zidere2021: Code<'static> = c("131533", "Zidere 2021");
pub const ZobelFilter: Code<'static> = c("130769", "Zobel filter");
#[deprecated = "retired in PS3.16"]
pub const ZonePercentage: Code<'static> = c("128833", "Zone Percentage");
#[deprecated = "retired in PS3.16"]
pub const ZoneSizeEntropy: Code<'static> = c("128836", "Zone Size Entropy");
#[deprecated = "retired in PS3.16"]
pub const ZoneSizeNonuniformity: Code<'static> = c("128831", "Zone Size Nonuniformity");
#[deprecated = "retired in PS3.16"]
pub const ZoneSizeNonuniformityNormalized: Code<'static> = c("128832", "Zone Size Nonuniformity Normalized");
#[deprecated = "retired in PS3.16"]
pub const ZoneSizeVariance: Code<'static> = c("128835", "Zone Size Variance");
pub fn by_name(name: &str) -> Option<Code<'static>> {
    super::find_named(&NAMED, name)
}
#[allow(deprecated)]
static NAMED: [(&str, Code<'static>); 5164] = [
    ("AAPM204APDimension", AAPM204APDimension),
    ("AAPM204EffectiveDiameterEstimatedFromPatientAge", AAPM204EffectiveDiameterEstimatedFromPatientAge),
    ("AAPM204LateralDimension", AAPM204LateralDimension),
    ("AAPM204SumOfLateralAndAPDimension", AAPM204SumOfLateralAndAPDimension),
    ("AAPM220", AAPM220),
    ("AAPMTG43", AAPMTG43),
    ("ACDConstant", ACDConstant),
    ("ACRAccreditationPhantomCT", ACRAccreditationPhantomCT),
    ("ACRAccreditationPhantomECT", ACRAccreditationPhantomECT),
    ("ACRAccreditationPhantomECTPerPET", ACRAccreditationPhantomECTPerPET),
    ("ACRAccreditationPhantomMR", ACRAccreditationPhantomMR),
    ("ACRAccreditationPhantomMammography", ACRAccreditationPhantomMammography),
    ("ACRAccreditationPhantomPET", ACRAccreditationPhantomPET),
    ("ACRAccreditationPhantomPETFaceplate", ACRAccreditationPhantomPETFaceplate),
    ("ACRAccreditationPhantomStereotacticBreastBiopsy", ACRAccreditationPhantomStereotacticBreastBiopsy),
    ("ACRGuidelinePerformanceOfAdultCTColonography", ACRGuidelinePerformanceOfAdultCTColonography),
    ("ACRPositionStatement", ACRPositionStatement),
    ("ACRStandardCTMedicalPhysicsPerformanceMonitoring", ACRStandardCTMedicalPhysicsPerformanceMonitoring),
    ("ADCHyperintense", ADCHyperintense),
    ("ADCHypointense", ADCHypointense),
    ("AF10", AF10),
    ("AF9", AF9),
    ("AGN15099889Zr", AGN15099889Zr),
    ("AIBasedMAR", AIBasedMAR),
    ("AIC", AIC),
    ("AIFIgnored", AIFIgnored),
    ("AIN76", AIN76),
    ("AIN93G", AIN93G),
    ("AIN93M", AIN93M),
    ("ANGPattern", ANGPattern),
    ("AP45", AP45),
    ("APPlus45", APPlus45),
    ("ARWave", ARWave),
    ("ATSMCu60", ATSMCu60),
    ("ATSMCu61", ATSMCu61),
    ("ATSMCu62", ATSMCu62),
    ("AWavePeakPressure", AWavePeakPressure),
    ("AWavePressureAverage", AWavePressureAverage),
    ("AbdominalAttachmentOfUmbilicalCord", AbdominalAttachmentOfUmbilicalCord),
    ("AbdominalCompressionArch", AbdominalCompressionArch),
    ("AbdominalCompressionBelt", AbdominalCompressionBelt),
    ("AbdominalCompressionBeltLength", AbdominalCompressionBeltLength),
    ("AbdominalCompressionBeltPressure", AbdominalCompressionBeltPressure),
    ("AbdominalCompressionPlatePositionNumber", AbdominalCompressionPlatePositionNumber),
    ("AbdominalImagingSpecialty", AbdominalImagingSpecialty),
    ("AbdominalRadiology", AbdominalRadiology),
    ("AbdominalRespiratoryEffort", AbdominalRespiratoryEffort),
    ("AbdominalRespiratoryInductance", AbdominalRespiratoryInductance),
    ("AbdominalRespiratoryPVDF", AbdominalRespiratoryPVDF),
    ("AblationOff", AblationOff),
    ("AblationOn", AblationOn),
    ("AbnormalCalcifications", AbnormalCalcifications),
    ("AbnormalDischarge", AbnormalDischarge),
    ("AbnormalDistributionOfAnatomicStructure", AbnormalDistributionOfAnatomicStructure),
    ("AbnormalInterstitialPattern", AbnormalInterstitialPattern),
    ("AbnormalLines1D", AbnormalLines1D),
    ("AbnormalLucency", AbnormalLucency),
    ("AbnormalOpacity", AbnormalOpacity),
    ("AbnormalRegion", AbnormalRegion),
    ("AbnormalTexture", AbnormalTexture),
    ("AbnormallyHighSensitivity", AbnormallyHighSensitivity),
    ("AbruptDuctTermination", AbruptDuctTermination),
    ("AbruptInterface", AbruptInterface),
    ("Absent", Absent),
    ("AbsoluteOphthalmicThickness", AbsoluteOphthalmicThickness),
    ("AbsoluteRegionalBloodFlow", AbsoluteRegionalBloodFlow),
    ("AbsoluteRegionalBloodVolume", AbsoluteRegionalBloodVolume),
    ("AbsoluteValuesOfVentricularMeasurements", AbsoluteValuesOfVentricularMeasurements),
    ("AbsorbedDose", AbsorbedDose),
    ("AcceptableUncertaintyInDoseResult", AcceptableUncertaintyInDoseResult),
    ("AccessControlDecision", AccessControlDecision),
    ("AccessionNumber", AccessionNumber),
    ("AccessoryClearanceInterlock", AccessoryClearanceInterlock),
    ("AccessoryTray", AccessoryTray),
    ("AccumulatedAverageGlandularDose", AccumulatedAverageGlandularDose),
    ("AccumulatedCTDIvolForwardEstimate", AccumulatedCTDIvolForwardEstimate),
    ("AccumulatedDLPForwardEstimate", AccumulatedDLPForwardEstimate),
    ("AccumulatedDoseData", AccumulatedDoseData),
    ("AccumulatedXRayDoseData", AccumulatedXRayDoseData),
    ("AcetabularAngle", AcetabularAngle),
    ("AcetabularCenterOfRotation", AcetabularCenterOfRotation),
    ("AcetabularCupInsert", AcetabularCupInsert),
    ("AcetabularCupMonoblock", AcetabularCupMonoblock),
    ("AcetabularCupShell", AcetabularCupShell),
    ("Acinar", Acinar),
    ("AcquiredImage", AcquiredImage),
    ("AcquiredVolume", AcquiredVolume),
    ("AcquisitionDate", AcquisitionDate),
    ("AcquisitionDateTime", AcquisitionDateTime),
    ("AcquisitionDeviceID", AcquisitionDeviceID),
    ("AcquisitionDeviceType", AcquisitionDeviceType),
    ("AcquisitionDoseAreaProductTotal", AcquisitionDoseAreaProductTotal),
    ("AcquisitionDoseRPTotal", AcquisitionDoseRPTotal),
    ("AcquisitionDuration", AcquisitionDuration),
    ("AcquisitionEquipment", AcquisitionEquipment),
    ("AcquisitionEquipmentAlignment", AcquisitionEquipmentAlignment),
    ("AcquisitionFramesCorrespondingToVolume", AcquisitionFramesCorrespondingToVolume),
    ("AcquisitionInitiationAfterEndOfRadiation", AcquisitionInitiationAfterEndOfRadiation),
    ("AcquisitionInitiationBeforeStartOfRadiation", AcquisitionInitiationBeforeStartOfRadiation),
    ("AcquisitionInitiationByTriggeringParameter", AcquisitionInitiationByTriggeringParameter),
    ("AcquisitionInitiationNotDefined", AcquisitionInitiationNotDefined),
    ("AcquisitionInitiationType", AcquisitionInitiationType),
    ("AcquisitionNumber", AcquisitionNumber),
    ("AcquisitionPlane", AcquisitionPlane),
    ("AcquisitionProtocol", AcquisitionProtocol),
    ("AcquisitionTime", AcquisitionTime),
    ("ActionOnFluid", ActionOnFluid),
    ("ActiveIngredientUndilutedConcentration", ActiveIngredientUndilutedConcentration),
    ("ActiveSurveillanceOfProstateCancer", ActiveSurveillanceOfProstateCancer),
    ("ActivityMeasurementDevice", ActivityMeasurementDevice),
    ("ActivityWorkload", ActivityWorkload),
    ("AcuteMIPresent", AcuteMIPresent),
    ("AcuteUnilateralRenalBlockage", AcuteUnilateralRenalBlockage),
    ("AdaptiveFiltering", AdaptiveFiltering),
    ("AdaptiveRadiationTherapyNote", AdaptiveRadiationTherapyNote),
    ("AddAddendumToReport", AddAddendumToReport),
    ("AddIntermediate", AddIntermediate),
    ("AddNear", AddNear),
    ("AddOther", AddOther),
    ("AddendedReport", AddendedReport),
    ("Addendum", Addendum),
    ("AdditionalEvaluationRequestedFromAbnormalScreeningStudy", AdditionalEvaluationRequestedFromAbnormalScreeningStudy),
    ("AdditionalEvaluationRequestedFromPriorStudy", AdditionalEvaluationRequestedFromPriorStudy),
    ("AdditionalProjections", AdditionalProjections),
    ("AdditionalTeachingFileInformation", AdditionalTeachingFileInformation),
    ("Adenolipoma", Adenolipoma),
    ("Adenomyoepithelioma", Adenomyoepithelioma),
    ("AdequatelyStained", AdequatelyStained),
    ("AdhocMeasurements", AdhocMeasurements),
    ("AdiabaticTissueHomogeneityModel", AdiabaticTissueHomogeneityModel),
    ("AdministeredActivity", AdministeredActivity),
    ("AdministrationDiscontinued", AdministrationDiscontinued),
    ("AdministrationMode", AdministrationMode),
    ("AdministrationStepType", AdministrationStepType),
    ("AdministratorOfRadiologyDepartment", AdministratorOfRadiologyDepartment),
    ("AdultCongenitalCardiacUltrasoundReport", AdultCongenitalCardiacUltrasoundReport),
    ("AdultEchocardiographyProcedureReport", AdultEchocardiographyProcedureReport),
    ("AdvancedLevel", AdvancedLevel),
    ("AdventitialDissection", AdventitialDissection),
    ("AdverseEventDetectionDateTime", AdverseEventDetectionDateTime),
    ("AerobicIndex", AerobicIndex),
    ("AgatstonScoreThreshold", AgatstonScoreThreshold),
    ("AgatstonScoringMethod", AgatstonScoringMethod),
    ("AgeAtFirstFullTermPregnancy", AgeAtFirstFullTermPregnancy),
    ("AgeAtMenopause", AgeAtMenopause),
    ("AgeAtOccurrence", AgeAtOccurrence),
    ("AgeCorrected", AgeCorrected),
    ("AgeEnded", AgeEnded),
    ("AgeStarted", AgeStarted),
    ("AgeWhenFirstMenstrualPeriodOccurred", AgeWhenFirstMenstrualPeriodOccurred),
    ("AgeWhenHysterectomyPerformed", AgeWhenHysterectomyPerformed),
    ("AgeWhenLeftOvaryRemoved", AgeWhenLeftOvaryRemoved),
    ("AgeWhenRightOvaryRemoved", AgeWhenRightOvaryRemoved),
    ("AgreementWithPriorFindings", AgreementWithPriorFindings),
    ("Air", Air),
    ("AirBronchiologram", AirBronchiologram),
    ("AirBronchogram", AirBronchogram),
    ("AirChanges", AirChanges),
    ("AirCrescent", AirCrescent),
    ("AirDetected", AirDetected),
    ("AirFluidLevel", AirFluidLevel),
    ("AirKermaAtOutputMeasurementPoint", AirKermaAtOutputMeasurementPoint),
    ("AirSpace", AirSpace),
    ("Airflow", Airflow),
    ("AirflowCPAP", AirflowCPAP),
    ("AirflowNasalProng", AirflowNasalProng),
    ("AirflowPVDF", AirflowPVDF),
    ("AirflowThermistor", AirflowThermistor),
    ("AirflowThermocouple", AirflowThermocouple),
    ("AirwayManagementMethod", AirwayManagementMethod),
    ("AirwayManagementSet", AirwayManagementSet),
    ("AirwayPartiallyObstructed", AirwayPartiallyObstructed),
    ("AirwaySeverelyObstructed", AirwaySeverelyObstructed),
    ("AirwaySubManagementMethod", AirwaySubManagementMethod),
    ("AirwayUnobstructed", AirwayUnobstructed),
    ("AlgorithmFamily", AlgorithmFamily),
    ("AlgorithmManufacturer", AlgorithmManufacturer),
    ("AlgorithmName", AlgorithmName),
    ("AlgorithmParameters", AlgorithmParameters),
    ("AlgorithmVersion", AlgorithmVersion),
    ("AlgorithmicAttenuationCorrection", AlgorithmicAttenuationCorrection),
    ("AllAlgorithmsSucceededWithFindings", AllAlgorithmsSucceededWithFindings),
    ("AllAlgorithmsSucceededWithoutFindings", AllAlgorithmsSucceededWithoutFindings),
    ("AllPlanes", AllPlanes),
    ("AlphaGEUDValue", AlphaGEUDValue),
    ("AlternatePhotometricInterpretationImage", AlternatePhotometricInterpretationImage),
    ("AlternateSOPClassInstance", AlternateSOPClassInstance),
    ("AlternativeDoseAlertBehaviorActive", AlternativeDoseAlertBehaviorActive),
    ("AmbientRoomEnvironment", AmbientRoomEnvironment),
    ("AmbiguousSex", AmbiguousSex),
    ("AmericaKennelClub", AmericaKennelClub),
    ("AmericanCanineAssociation", AmericanCanineAssociation),
    ("AmericanPurebredRegistry", AmericanPurebredRegistry),
    ("AmericanRareBreedAssociation", AmericanRareBreedAssociation),
    ("AmericasPetRegistryInc", AmericasPetRegistryInc),
    ("AnalysisPerformed", AnalysisPerformed),
    ("AnalysisQualityRating", AnalysisQualityRating),
    ("AnalyticalAlgorithm", AnalyticalAlgorithm),
    ("AnatomicIdentifier", AnatomicIdentifier),
    ("AnatomicImage", AnatomicImage),
    ("AnatomicNonColon", AnatomicNonColon),
    ("AnatomicalAxisOfFemur", AnatomicalAxisOfFemur),
    ("AnatomyCoverageIncomplete", AnatomyCoverageIncomplete),
    ("Anechoic", Anechoic),
    ("AnesthesiaCategory", AnesthesiaCategory),
    ("AnesthesiaMethod", AnesthesiaMethod),
    ("AnesthesiaMethodSet", AnesthesiaMethodSet),
    ("AnesthesiaSubCategory", AnesthesiaSubCategory),
    ("AneurysmOnCitedVessel", AneurysmOnCitedVessel),
    ("AnginalEquivalent", AnginalEquivalent),
    ("AngioplastyBalloonDeflated", AngioplastyBalloonDeflated),
    ("AngioplastyBalloonInflated", AngioplastyBalloonInflated),
    ("Angiosarcoma", Angiosarcoma),
    ("Angioscopy", Angioscopy),
    ("Angle", Angle),
    ("AngularMargins", AngularMargins),
    ("AngularModulation", AngularModulation),
    ("AngularSecondMomentOfGLCM", AngularSecondMomentOfGLCM),
    ("AngularViewingEvaluation", AngularViewingEvaluation),
    ("AnimalHandlingDuringSpecifiedPhase", AnimalHandlingDuringSpecifiedPhase),
    ("AnimalHousing", AnimalHousing),
    ("AnimalHousingRoom", AnimalHousingRoom),
    ("AnimalRegistryUnlimited", AnimalRegistryUnlimited),
    ("AnimalResearchFoundation", AnimalResearchFoundation),
    ("AnimalRoomIdentifier", AnimalRoomIdentifier),
    ("AnimalRoomType", AnimalRoomType),
    ("Annotation", Annotation),
    ("AnnotationNote", AnnotationNote),
    ("AnodeTargetMaterial", AnodeTargetMaterial),
    ("AnomalousExponentParameter", AnomalousExponentParameter),
    ("AntegradeDuctusVenosus", AntegradeDuctusVenosus),
    ("AnteriorFirst", AnteriorFirst),
    ("AnteriorInferiorAxis", AnteriorInferiorAxis),
    ("AnteriorJunctionLine", AnteriorJunctionLine),
    ("AnteriorPosterior", AnteriorPosterior),
    ("AnteriorPosteriorAxis", AnteriorPosteriorAxis),
    ("AnteriorToInferior", AnteriorToInferior),
    ("AnteriorToPosterior", AnteriorToPosterior),
    ("AnterolateralPapillaryMuscleToTheLeftTrigone", AnterolateralPapillaryMuscleToTheLeftTrigone),
    ("AnthropomorphicModel", AnthropomorphicModel),
    ("AntiB22089Zr", AntiB22089Zr),
    ("AntralFollicle", AntralFollicle),
    ("AnyDecisionToBiopsyShouldBeBasedOnClinicalAssessment", AnyDecisionToBiopsyShouldBeBasedOnClinicalAssessment),
    ("AortaSinotubularJunctionArea", AortaSinotubularJunctionArea),
    ("AorticAnnulusCalcificationSeverity", AorticAnnulusCalcificationSeverity),
    ("AorticAnnulusMaxDiameter", AorticAnnulusMaxDiameter),
    ("AorticAnnulusMinDiameter", AorticAnnulusMinDiameter),
    ("AorticAnnulusPerimeter", AorticAnnulusPerimeter),
    ("AorticArchView", AorticArchView),
    ("AorticCalcificationVolume", AorticCalcificationVolume),
    ("AorticCommissuresCalcificationSeverity", AorticCommissuresCalcificationSeverity),
    ("AorticIntervention", AorticIntervention),
    ("AorticKnob", AorticKnob),
    ("AorticRootHeight", AorticRootHeight),
    ("AorticSinotubularJunctionDiameter", AorticSinotubularJunctionDiameter),
    ("AorticSinusOfValsalvaArea", AorticSinusOfValsalvaArea),
    ("AorticValveClosure", AorticValveClosure),
    ("AorticValveCoaptationLength", AorticValveCoaptationLength),
    ("AorticValveLeftCoronaryLeafletHeight", AorticValveLeftCoronaryLeafletHeight),
    ("AorticValveLeftCoronaryLeafletLength", AorticValveLeftCoronaryLeafletLength),
    ("AorticValveLeftLeafletIntercommissuralAngle", AorticValveLeftLeafletIntercommissuralAngle),
    ("AorticValveLeftLeafletIntercommissuralDistance", AorticValveLeftLeafletIntercommissuralDistance),
    ("AorticValveNoncoronaryLeafletHeight", AorticValveNoncoronaryLeafletHeight),
    ("AorticValveNoncoronaryLeafletIntercommissuralAngle", AorticValveNoncoronaryLeafletIntercommissuralAngle),
    ("AorticValveNoncoronaryLeafletIntercommissuralDistance", AorticValveNoncoronaryLeafletIntercommissuralDistance),
    ("AorticValveNoncoronaryLeafletLength", AorticValveNoncoronaryLeafletLength),
    ("AorticValveOpening", AorticValveOpening),
    ("AorticValveRightCoronaryLeafletHeight", AorticValveRightCoronaryLeafletHeight),
    ("AorticValveRightCoronaryLeafletLength", AorticValveRightCoronaryLeafletLength),
    ("AorticValveRightLeafletIntercommissuralAngle", AorticValveRightLeafletIntercommissuralAngle),
    ("AorticValveRightLeafletIntercommissuralDistance", AorticValveRightLeafletIntercommissuralDistance),
    ("AortoMitralInterAnnularAngle", AortoMitralInterAnnularAngle),
    ("ApertureBlock", ApertureBlock),
    ("ApexBaseAxis", ApexBaseAxis),
    ("ApexToBase", ApexToBase),
    ("ApicalFourChamberRVFocused", ApicalFourChamberRVFocused),
    ("ApicalFourChamberRVModified", ApicalFourChamberRVModified),
    ("ApparentDiffusionCoefficient", ApparentDiffusionCoefficient),
    ("ApparentKurtosisCoefficient", ApparentKurtosisCoefficient),
    ("Application", Application),
    ("ApplicationActivity", ApplicationActivity),
    ("ApplicationLauncher", ApplicationLauncher),
    ("ApplicationStart", ApplicationStart),
    ("ApplicationStop", ApplicationStop),
    ("ApplicatorPresent", ApplicatorPresent),
    ("AppropriateForTheDevice", AppropriateForTheDevice),
    ("AppropriateForTheIndications", AppropriateForTheIndications),
    ("ApprovedForExperimentalUse", ApprovedForExperimentalUse),
    ("ApprovedForUseAtTheInstitution", ApprovedForUseAtTheInstitution),
    ("ApprovedForUseInTheClinicalTrial", ApprovedForUseInTheClinicalTrial),
    ("ApprovedForUseOnPregnantPatients", ApprovedForUseOnPregnantPatients),
    ("ApproximateSpatialLocation", ApproximateSpatialLocation),
    ("ArcBeam", ArcBeam),
    ("ArcOfCalcium", ArcOfCalcium),
    ("ArchOfTheAzygosVein", ArchOfTheAzygosVein),
    ("ArchitecturalDistortion", ArchitecturalDistortion),
    ("Archive", Archive),
    ("AreaBasedMethod", AreaBasedMethod),
    ("AreaByPressureHalfTime", AreaByPressureHalfTime),
    ("AreaEqualsFlowPer445SqrtGradientMmHg", AreaEqualsFlowPer445SqrtGradientMmHg),
    ("AreaLengthBiplane", AreaLengthBiplane),
    ("AreaLengthDodge", AreaLengthDodge),
    ("AreaLengthKennedy", AreaLengthKennedy),
    ("AreaLengthMethod", AreaLengthMethod),
    ("AreaLengthSinglePlane", AreaLengthSinglePlane),
    ("AreaLengthWynne", AreaLengthWynne),
    ("AreaOfAClosedNURBS", AreaOfAClosedNURBS),
    ("AreaOfClosedIrregularPolygon", AreaOfClosedIrregularPolygon),
    ("AreaOfDefinedRegion", AreaOfDefinedRegion),
    ("AreaOfVascularCalcification", AreaOfVascularCalcification),
    ("AreaOutline", AreaOutline),
    ("AreaOutline_121201", AreaOutline_121201),
    ("AreaSymmetry", AreaSymmetry),
    ("ArithmeticAverageOfDwZ", ArithmeticAverageOfDwZ),
    ("ArithmeticAverageOfSSDEZ", ArithmeticAverageOfSSDEZ),
    ("ArmOfAngle", ArmOfAngle),
    ("ArterialConduitAngiography", ArterialConduitAngiography),
    ("ArterialDopplerScore", ArterialDopplerScore),
    ("ArterialPulseWaveform", ArterialPulseWaveform),
    ("ArteriovenousDifference", ArteriovenousDifference),
    ("ArtifactSOtherThanGridOrDetectorArtifact", ArtifactSOtherThanGridOrDetectorArtifact),
    ("ArtificialIntelligence", ArtificialIntelligence),
    ("AscendingAortaDiameter", AscendingAortaDiameter),
    ("AspenChipBedding", AspenChipBedding),
    ("AspenShavingBedding", AspenShavingBedding),
    ("Assembly", Assembly),
    ("AssessChange", AssessChange),
    ("AssessmentByComparison", AssessmentByComparison),
    ("AssessmentByRules", AssessmentByRules),
    ("AssessmentCategory", AssessmentCategory),
    ("Assisting", Assisting),
    ("AssociatedChestComponent", AssociatedChestComponent),
    ("AsymmetricImplants", AsymmetricImplants),
    ("AsynchronousInvolution", AsynchronousInvolution),
    ("AtLastAppointment", AtLastAppointment),
    ("AtSurfaceOfPatient", AtSurfaceOfPatient),
    ("AtomicNumber", AtomicNumber),
    ("AtrialDiastolicFillingDWave", AtrialDiastolicFillingDWave),
    ("AtrialPressureMeasurements", AtrialPressureMeasurements),
    ("AtrioventricularTimeInterval", AtrioventricularTimeInterval),
    ("Attach", Attach),
    ("AttendingConsultant", AttendingConsultant),
    ("AttenuationCoefficient", AttenuationCoefficient),
    ("AttenuationCorrection", AttenuationCorrection),
    ("AttenuatorCategory", AttenuatorCategory),
    ("AttenuatorCharacteristics", AttenuatorCharacteristics),
    ("AttenuatorDescription", AttenuatorDescription),
    ("AttenuatorPosition", AttenuatorPosition),
    ("Attenuators", Attenuators),
    ("Audio", Audio),
    ("AuditLogUsed", AuditLogUsed),
    ("AuditRecordingStarted", AuditRecordingStarted),
    ("AuditRecordingStopped", AuditRecordingStopped),
    ("AuthenticationDecision", AuthenticationDecision),
    ("AuthorizationDecision", AuthorizationDecision),
    ("AutoKeratometry", AutoKeratometry),
    ("AutocorrelationOfGLCM", AutocorrelationOfGLCM),
    ("AutomatedAdministration", AutomatedAdministration),
    ("AutomatedDataCollection", AutomatedDataCollection),
    ("AutomatedManualInjectPhase", AutomatedManualInjectPhase),
    ("AutomatedOptical", AutomatedOptical),
    ("AutomatedSlideStainer", AutomatedSlideStainer),
    ("AutomaticProgrammedAdministrationPhase", AutomaticProgrammedAdministrationPhase),
    ("AutomaticProgrammedDelayPhase", AutomaticProgrammedDelayPhase),
    ("AutomaticProgrammedWaitPhase", AutomaticProgrammedWaitPhase),
    ("AutomaticWithManualHoldPhase", AutomaticWithManualHoldPhase),
    ("AutomaticallyDetectedAIFROI", AutomaticallyDetectedAIFROI),
    ("Autorefraction", Autorefraction),
    ("AutorefractionMeasurementsSOPInstance", AutorefractionMeasurementsSOPInstance),
    ("AutorefractionVisualAcuity", AutorefractionVisualAcuity),
    ("AvascularComplexFlow", AvascularComplexFlow),
    ("AvascularComplexMap", AvascularComplexMap),
    ("AverageBreastFeedingPeriod", AverageBreastFeedingPeriod),
    ("AverageCalcificationDistanceInACalcificationCluster", AverageCalcificationDistanceInACalcificationCluster),
    ("AverageGanglionCellThickness", AverageGanglionCellThickness),
    ("AverageGanglionCellThicknessInPosteriorPoleGrid", AverageGanglionCellThicknessInPosteriorPoleGrid),
    ("AverageGanglionCellThicknessInferiorSector", AverageGanglionCellThicknessInferiorSector),
    ("AverageGanglionCellThicknessNasalInferiorSector", AverageGanglionCellThicknessNasalInferiorSector),
    ("AverageGanglionCellThicknessNasalSector", AverageGanglionCellThicknessNasalSector),
    ("AverageGanglionCellThicknessNasalSuperiorSector", AverageGanglionCellThicknessNasalSuperiorSector),
    ("AverageGanglionCellThicknessSuperiorSector", AverageGanglionCellThicknessSuperiorSector),
    ("AverageGanglionCellThicknessTemporalInferiorSector", AverageGanglionCellThicknessTemporalInferiorSector),
    ("AverageGanglionCellThicknessTemporalSector", AverageGanglionCellThicknessTemporalSector),
    ("AverageGanglionCellThicknessTemporalSuperiorSector", AverageGanglionCellThicknessTemporalSuperiorSector),
    ("AverageGlandularDose", AverageGlandularDose),
    ("AverageMacularThickness", AverageMacularThickness),
    ("AverageXRayTubeCurrent", AverageXRayTubeCurrent),
    ("AvoidanceVolume", AvoidanceVolume),
    ("AxialDiffusivity", AxialDiffusivity),
    ("AxialKurtosis", AxialKurtosis),
    ("AxialMeasurementsSOPInstance", AxialMeasurementsSOPInstance),
    ("AxillaryNodalMetastases", AxillaryNodalMetastases),
    ("AxillaryNodeHyperplasia", AxillaryNodeHyperplasia),
    ("AxillaryNodeWithCalcifications", AxillaryNodeWithCalcifications),
    ("AxillaryNodeWithLymphoma", AxillaryNodeWithLymphoma),
    ("AzygoesophagealRecessInterface", AzygoesophagealRecessInterface),
    ("BIC", BIC),
    ("BMIEqualsWtPerHt2", BMIEqualsWtPerHt2),
    ("BN01Pattern", BN01Pattern),
    ("BN02Pattern", BN02Pattern),
    ("BN03Pattern", BN03Pattern),
    ("BN04Pattern", BN04Pattern),
    ("BN05Pattern", BN05Pattern),
    ("BN06Pattern", BN06Pattern),
    ("BN07Pattern", BN07Pattern),
    ("BN08Pattern", BN08Pattern),
    ("BN09Pattern", BN09Pattern),
    ("BN10Pattern", BN10Pattern),
    ("BN11Pattern", BN11Pattern),
    ("BN12Pattern", BN12Pattern),
    ("BN13Pattern", BN13Pattern),
    ("BN14Pattern", BN14Pattern),
    ("BN15Pattern", BN15Pattern),
    ("BN16Pattern", BN16Pattern),
    ("BN17Pattern", BN17Pattern),
    ("BN18Pattern", BN18Pattern),
    ("BRCA1BreastCancerGene", BRCA1BreastCancerGene),
    ("BRCA2BreastCancerGene", BRCA2BreastCancerGene),
    ("BRCA3BreastCancerGene", BRCA3BreastCancerGene),
    ("BREPRadiationTransportModel", BREPRadiationTransportModel),
    ("BSA1219", BSA1219),
    ("BSAEquals00004688WT0816800154LogWT", BSAEquals00004688WT0816800154LogWT),
    ("BSAEquals0003207WT0728500188LogWTHT03", BSAEquals0003207WT0728500188LogWTHT03),
    ("BSAEquals0007184WT0425HT0725", BSAEquals0007184WT0425HT0725),
    ("BSAEquals0007358WT0425HT0725", BSAEquals0007358WT0425HT0725),
    ("BSAEquals0008883WT0444HT0663", BSAEquals0008883WT0444HT0663),
    ("BSAEquals0009568WT0473HT0655", BSAEquals0009568WT0473HT0655),
    ("BSAEquals0010265WT0423HT0651", BSAEquals0010265WT0423HT0651),
    ("BSAEquals00235WT051456HT042246", BSAEquals00235WT051456HT042246),
    ("BSAEquals0024265WT05378HT03964", BSAEquals0024265WT05378HT03964),
    ("BSAEquals0038189WT0423HT0362", BSAEquals0038189WT0423HT0362),
    ("BSAEquals1321Plus03433WT", BSAEquals1321Plus03433WT),
    ("BSAEqualsHTWTPer3605", BSAEqualsHTWTPer3605),
    ("BackOfCorneaToFrontOfLens", BackOfCorneaToFrontOfLens),
    ("Background", Background),
    ("BackgroundForLesionToBackgroundSUVRatio", BackgroundForLesionToBackgroundSUVRatio),
    ("BackrestPitchAngle", BackrestPitchAngle),
    ("Backscatter", Backscatter),
    ("BallAndDiskTypeLeftAtrialAppendageClosureDevice", BallAndDiskTypeLeftAtrialAppendageClosureDevice),
    ("BallTypeLeftAtrialAppendageClosureDevice", BallTypeLeftAtrialAppendageClosureDevice),
    ("BarcodeValue", BarcodeValue),
    ("BarrettDesignFactor", BarrettDesignFactor),
    ("BarrettKeratoconus", BarrettKeratoconus),
    ("BarrettLensFactor", BarrettLensFactor),
    ("BarrettRx", BarrettRx),
    ("BarrettToric", BarrettToric),
    ("BarrettTrueK", BarrettTrueK),
    ("BarrettTrueKToric", BarrettTrueKToric),
    ("BarrettUniversalII", BarrettUniversalII),
    ("BasalCellCarcinomaOfNipple", BasalCellCarcinomaOfNipple),
    ("BaseToApex", BaseToApex),
    ("Baseline", Baseline),
    ("BaselineCategory", BaselineCategory),
    ("BaselineResultCorrection", BaselineResultCorrection),
    ("BaselineScreeningMammogram", BaselineScreeningMammogram),
    ("BaselineStenosisMeasurement", BaselineStenosisMeasurement),
    ("BaselineTIMIFlow", BaselineTIMIFlow),
    ("BasicApplicationConfidentialityProfile", BasicApplicationConfidentialityProfile),
    ("BasisImageDecomposition", BasisImageDecomposition),
    ("BasisImageForSpectralProcessing", BasisImageForSpectralProcessing),
    ("BatsWingDistribution", BatsWingDistribution),
    ("BayesianProbability", BayesianProbability),
    ("BazettQTcAlgorithm", BazettQTcAlgorithm),
    ("BeadedSeptumSign", BeadedSeptumSign),
    ("BeamBlock", BeamBlock),
    ("BeamEnergyInterlock", BeamEnergyInterlock),
    ("BeamPosition", BeamPosition),
    ("BeamShapingNote", BeamShapingNote),
    ("BeamTargetingToleranceViolation", BeamTargetingToleranceViolation),
    ("BeatDetectedAccepted", BeatDetectedAccepted),
    ("BeatDetectedRejected", BeatDetectedRejected),
    ("BeatNumber", BeatNumber),
    ("BeddingDepth", BeddingDepth),
    ("BeddingManufacturer", BeddingManufacturer),
    ("BeddingMass", BeddingMass),
    ("BeddingProductCode", BeddingProductCode),
    ("BeddingProductName", BeddingProductName),
    ("BeddingVolume", BeddingVolume),
    ("BeginCirculatorySupport", BeginCirculatorySupport),
    ("BeginOxygenAdministration", BeginOxygenAdministration),
    ("BeginPacing", BeginPacing),
    ("BeginTransferringDICOMInstances", BeginTransferringDICOMInstances),
    ("BeginVentilation", BeginVentilation),
    ("BeginningOfTimePeriod", BeginningOfTimePeriod),
    ("BehaviorOfTheFindingSite", BehaviorOfTheFindingSite),
    ("BenignCalcifications", BenignCalcifications),
    ("BenignCystWithBlood", BenignCystWithBlood),
    ("Benralizumab89Zr", Benralizumab89Zr),
    ("BesselFilter", BesselFilter),
    ("BestIllustrationOfFinding", BestIllustrationOfFinding),
    ("BestInSet", BestInSet),
    ("BestOverallResponse", BestOverallResponse),
    ("Bevacizumab89Zr", Bevacizumab89Zr),
    ("BiExponentialIVIMDiffusionModel", BiExponentialIVIMDiffusionModel),
    ("BiPlaneAngiography", BiPlaneAngiography),
    ("BillingCode", BillingCode),
    ("BinarySegmentation", BinarySegmentation),
    ("BiomagneticImaging", BiomagneticImaging),
    ("BiomedicalEngineering", BiomedicalEngineering),
    ("BiometryGroup", BiometryGroup),
    ("BiophysicalProfile", BiophysicalProfile),
    ("BiopsyLocalizer", BiopsyLocalizer),
    ("BiopsyShouldBeConsidered", BiopsyShouldBeConsidered),
    ("BiopsyShouldBeStronglyConsidered", BiopsyShouldBeStronglyConsidered),
    ("Bioptome", Bioptome),
    ("BiosafetyConditions", BiosafetyConditions),
    ("BiplaneEllipse", BiplaneEllipse),
    ("BiquadFilter", BiquadFilter),
    ("Blinatumomab89Zr", Blinatumomab89Zr),
    ("BlindEstimationOfAIF", BlindEstimationOfAIF),
    ("BlindSpotMonitoring", BlindSpotMonitoring),
    ("BlockPresent", BlockPresent),
    ("BloodCarbonDioxideSaturation", BloodCarbonDioxideSaturation),
    ("BloodDeoxygenationLevel", BloodDeoxygenationLevel),
    ("BloodLabMeasurements", BloodLabMeasurements),
    ("BloodOxygenContent", BloodOxygenContent),
    ("BloodOxygenationLevel", BloodOxygenationLevel),
    ("BloodTemperature", BloodTemperature),
    ("BloodTingedColor", BloodTingedColor),
    ("BloodVelocityMean", BloodVelocityMean),
    ("BloodVelocityMeasurements", BloodVelocityMeasurements),
    ("BloodVelocityMinimum", BloodVelocityMinimum),
    ("BloodVelocityPeak", BloodVelocityPeak),
    ("BloodVelocityVsTimeOfCardiacCycle", BloodVelocityVsTimeOfCardiacCycle),
    ("BloodVesselVascularInvasion", BloodVesselVascularInvasion),
    ("BloomRichardsonGrade", BloomRichardsonGrade),
    ("BlueFilter", BlueFilter),
    ("BlueGreenFilter", BlueGreenFilter),
    ("Boak", Boak),
    ("BodeFilter", BodeFilter),
    ("BodyFrame", BodyFrame),
    ("BodyHabitusAttenuation", BodyHabitusAttenuation),
    ("BodyNodeSet", BodyNodeSet),
    ("Bolting", Bolting),
    ("BolusArrivalTime", BolusArrivalTime),
    ("BolusPresent", BolusPresent),
    ("BolusShapingCurve", BolusShapingCurve),
    ("BoneAndSoftTissue", BoneAndSoftTissue),
    ("BoneMineralDensitometry", BoneMineralDensitometry),
    ("BoothbyTable", BoothbyTable),
    ("Bootstrap", Bootstrap),
    ("BorderDefinition", BorderDefinition),
    ("BorderShape", BorderShape),
    ("Borderline", Borderline),
    ("BorderlineAndGeneralReductionInSensitivity", BorderlineAndGeneralReductionInSensitivity),
    ("BorgCR10Scale", BorgCR10Scale),
    ("BorgRPEScale", BorgRPEScale),
    ("BostonChildrensZscoreData", BostonChildrensZscoreData),
    ("BottomZLocationOfReconstructableVolume", BottomZLocationOfReconstructableVolume),
    ("BottomZLocationOfScanningLength", BottomZLocationOfScanningLength),
    ("BoundaryPoint", BoundaryPoint),
    ("BowtieFilter", BowtieFilter),
    ("BrachytherapyChannel", BrachytherapyChannel),
    ("BrachytherapyChannelShield", BrachytherapyChannelShield),
    ("BrachytherapyDevice", BrachytherapyDevice),
    ("BrachytherapyIsotope", BrachytherapyIsotope),
    ("BrachytherapySourceApplicator", BrachytherapySourceApplicator),
    ("BrandName", BrandName),
    ("BreakoutBoxConnected", BreakoutBoxConnected),
    ("BreakoutBoxDisconnected", BreakoutBoxDisconnected),
    ("BreastAttenuation", BreastAttenuation),
    ("BreastBackgroundEchotexture", BreastBackgroundEchotexture),
    ("BreastBoard", BreastBoard),
    ("BreastBridge", BreastBridge),
    ("BreastComposition", BreastComposition),
    ("BreastFeedingHistory", BreastFeedingHistory),
    ("BreastGeometry", BreastGeometry),
    ("BreastImagingReport", BreastImagingReport),
    ("BreastImagingSubjectMatter", BreastImagingSubjectMatter),
    ("BreastOutlineIncludingPectoralMuscleTissue", BreastOutlineIncludingPectoralMuscleTissue),
    ("BreastThickness", BreastThickness),
    ("BreastTissueDensity", BreastTissueDensity),
    ("Brentuximab89Zr", Brentuximab89Zr),
    ("BrightRedColor", BrightRedColor),
    ("BrightfieldIllumination", BrightfieldIllumination),
    ("Bronchovascular", Bronchovascular),
    ("BulletMethod", BulletMethod),
    ("ButterflyDistribution", ButterflyDistribution),
    ("ButterflyFilter", ButterflyFilter),
    ("ButterworthFilter", ButterworthFilter),
    ("C0InadequateStudyPerAwaitingPriorComparisons", C0InadequateStudyPerAwaitingPriorComparisons),
    ("C1NormalColonOrBenignLesion", C1NormalColonOrBenignLesion),
    ("C2IntermediatePolypOrIndeterminateFinding", C2IntermediatePolypOrIndeterminateFinding),
    ("C3PolypPossiblyAdvancedAdenoma", C3PolypPossiblyAdvancedAdenoma),
    ("C4ColonicMassLikelyMalignant", C4ColonicMassLikelyMalignant),
    ("CADOperatingPoint", CADOperatingPoint),
    ("CADOperatingPointDescription", CADOperatingPointDescription),
    ("CADOperatingPointTable", CADOperatingPointTable),
    ("CADProcessingAndFindingsSummary", CADProcessingAndFindingsSummary),
    ("CARDIOsphere", CARDIOsphere),
    ("CArmMount", CArmMount),
    ("CCNippleNotCenteredOnImage", CCNippleNotCenteredOnImage),
    ("CCNotAllMedialTissueVisualized", CCNotAllMedialTissueVisualized),
    ("CCPosteriorNippleLineDoesNotMeasureWithin1CmOfMLO", CCPosteriorNippleLineDoesNotMeasureWithin1CmOfMLO),
    ("CD", CD),
    ("CDImported", CDImported),
    ("CFXRegionInLAOProjection", CFXRegionInLAOProjection),
    ("CG250FAb289Zr", CG250FAb289Zr),
    ("CHARMED", CHARMED),
    ("CICFilter", CICFilter),
    ("CKDEPIFormulaEstimationOfGFR", CKDEPIFormulaEstimationOfGFR),
    ("CKMBBaseline", CKMBBaseline),
    ("CKMBPeak", CKMBPeak),
    ("CLR1404I124", CLR1404I124),
    ("CLR1404I131", CLR1404I131),
    ("CMAbU3689Zr", CMAbU3689Zr),
    ("CO2Respiration", CO2Respiration),
    ("CO2Transcutaneous", CO2Transcutaneous),
    ("CO2TrendEndTidalMainStream", CO2TrendEndTidalMainStream),
    ("CO2TrendEndTidalSideStream", CO2TrendEndTidalSideStream),
    ("CO2TrendMainStream", CO2TrendMainStream),
    ("CO2TrendSideStream", CO2TrendSideStream),
    ("CO2WaveformEndTidalMainStream", CO2WaveformEndTidalMainStream),
    ("CO2WaveformEndTidalSideStream", CO2WaveformEndTidalSideStream),
    ("CO2WaveformMainStream", CO2WaveformMainStream),
    ("CO2WaveformSideStream", CO2WaveformSideStream),
    ("CRPerDRMechanicalConfiguration", CRPerDRMechanicalConfiguration),
    ("CRTDisplay", CRTDisplay),
    ("CRTFrontProjectionSystem", CRTFrontProjectionSystem),
    ("CRTRearProjectionSystem", CRTRearProjectionSystem),
    ("CTAccumulatedDoseData", CTAccumulatedDoseData),
    ("CTAcquisition", CTAcquisition),
    ("CTAcquisitionParameters", CTAcquisitionParameters),
    ("CTAcquisitionType", CTAcquisitionType),
    ("CTBasedAttenuationCorrection", CTBasedAttenuationCorrection),
    ("CTDIfreeairCalculationFactor", CTDIfreeairCalculationFactor),
    ("CTDIfreeairToEConversionViaMCComputation", CTDIfreeairToEConversionViaMCComputation),
    ("CTDIfreeairToEConversionViaMeasurement", CTDIfreeairToEConversionViaMeasurement),
    ("CTDIvolAlertValue", CTDIvolAlertValue),
    ("CTDIvolAlertValueConfigured", CTDIvolAlertValueConfigured),
    ("CTDIvolForwardEstimate", CTDIvolForwardEstimate),
    ("CTDIvolNotificationValue", CTDIvolNotificationValue),
    ("CTDIvolNotificationValueConfigured", CTDIvolNotificationValueConfigured),
    ("CTDIwPhantomType", CTDIwPhantomType),
    ("CTDose", CTDose),
    ("CTDoseLengthProductSubTotal", CTDoseLengthProductSubTotal),
    ("CTDoseLengthProductTotal", CTDoseLengthProductTotal),
    ("CTEffectiveDoseTotal", CTEffectiveDoseTotal),
    ("CTImageReconstruction", CTImageReconstruction),
    ("CTProtocol", CTProtocol),
    ("CTVNodal", CTVNodal),
    ("CTVPrimary", CTVPrimary),
    ("CTXRaySourceParameters", CTXRaySourceParameters),
    ("CU3689Zr", CU3689Zr),
    ("CWavePressure", CWavePressure),
    ("CalcificationCluster", CalcificationCluster),
    ("CalcificationDescriptor", CalcificationDescriptor),
    ("CalcificationDistribution", CalcificationDistribution),
    ("CalcificationExtentAsPercentOfSurface", CalcificationExtentAsPercentOfSurface),
    ("CalcificationExtentAsPercentOfVolume", CalcificationExtentAsPercentOfVolume),
    ("CalcificationType", CalcificationType),
    ("CalcificationsOutsideOfAMass", CalcificationsOutsideOfAMass),
    ("CalcificationsSeenInTheCore", CalcificationsSeenInTheCore),
    ("CalcificationsWithinAMass", CalcificationsWithinAMass),
    ("CalcifiedImplant", CalcifiedImplant),
    ("Calcium", Calcium),
    ("CalciumLength", CalciumLength),
    ("CalciumMass", CalciumMass),
    ("CalciumMassThreshold", CalciumMassThreshold),
    ("CalciumScore", CalciumScore),
    ("CalciumScoringAnalysis", CalciumScoringAnalysis),
    ("CalciumScoringCalibration", CalciumScoringCalibration),
    ("CalciumVolume", CalciumVolume),
    ("Calculated", Calculated),
    ("Calculated_121428", Calculated_121428),
    ("CalculationDescription", CalculationDescription),
    ("CalculationFailure", CalculationFailure),
    ("Calibration", Calibration),
    ("CalibrationDateTime", CalibrationDateTime),
    ("CalibrationFactor", CalibrationFactor),
    ("CalibrationIntendedAcquisitionProtocol", CalibrationIntendedAcquisitionProtocol),
    ("CalibrationIntendedDoseIndex", CalibrationIntendedDoseIndex),
    ("CalibrationMethod", CalibrationMethod),
    ("CalibrationObject", CalibrationObject),
    ("CalibrationObjectSize", CalibrationObjectSize),
    ("CalibrationObjectUsed", CalibrationObjectUsed),
    ("CalibrationProtocol", CalibrationProtocol),
    ("CalibrationResponsibleParty", CalibrationResponsibleParty),
    ("CalibrationUncertainty", CalibrationUncertainty),
    ("CamellinCalossi", CamellinCalossi),
    ("CanadianBorderCollieAssociation", CanadianBorderCollieAssociation),
    ("CanadianKennelClub", CanadianKennelClub),
    ("CanadianLivestockRecordsAssociation", CanadianLivestockRecordsAssociation),
    ("CancerElsewhere", CancerElsewhere),
    ("CanineFederationOfCanada", CanineFederationOfCanada),
    ("CarcinomaInChildren", CarcinomaInChildren),
    ("CarcinomaInEctopicBreast", CarcinomaInEctopicBreast),
    ("CarcinomaInPregnancyAndLactation", CarcinomaInPregnancyAndLactation),
    ("CarcinomaWithEndocrineDifferentiation", CarcinomaWithEndocrineDifferentiation),
    ("CarcinomaWithMetaplasia", CarcinomaWithMetaplasia),
    ("Carcinosarcoma", Carcinosarcoma),
    ("CardiacAxis", CardiacAxis),
    ("CardiacAxisAngle", CardiacAxisAngle),
    ("CardiacElectrophysiology", CardiacElectrophysiology),
    ("CardiacFunctionScore", CardiacFunctionScore),
    ("CardiacImagingSpecialty", CardiacImagingSpecialty),
    ("CardiacStressState", CardiacStressState),
    ("CardiothoracicSizeRatioScore", CardiothoracicSizeRatioScore),
    ("CardiovascularAnalysisReport", CardiovascularAnalysisReport),
    ("CardiovascularImagingSubjectMatter", CardiovascularImagingSubjectMatter),
    ("CardiovascularRadiology", CardiovascularRadiology),
    ("CarinaAngle", CarinaAngle),
    ("CarrierGas", CarrierGas),
    ("CarrierTemperatureSensor", CarrierTemperatureSensor),
    ("CartilaginousAndOsseousChange", CartilaginousAndOsseousChange),
    ("CaseSensitivity", CaseSensitivity),
    ("CaseSpecificity", CaseSpecificity),
    ("CassetteBasedProjectionRadiographySystem", CassetteBasedProjectionRadiographySystem),
    ("Cast", Cast),
    ("CathLabProcedureLog", CathLabProcedureLog),
    ("CatheterCurve", CatheterCurve),
    ("CatheterSize", CatheterSize),
    ("CatheterizationProcedurePhase", CatheterizationProcedurePhase),
    ("Caudal10DegreeDistalCranioproximalOblique", Caudal10DegreeDistalCranioproximalOblique),
    ("CavityExtentAsPercentOfVolume", CavityExtentAsPercentOfVolume),
    ("CavityRadiationShield", CavityRadiationShield),
    ("Center", Center),
    ("CenterOfGravity", CenterOfGravity),
    ("CenterOfMass", CenterOfMass),
    ("CenterOfRotation", CenterOfRotation),
    ("CenterOfTableHead", CenterOfTableHead),
    ("Centerline", Centerline),
    ("CenterlineWallMotionAnalysis", CenterlineWallMotionAnalysis),
    ("CentralBreathingPosition", CentralBreathingPosition),
    ("CentralLine", CentralLine),
    ("Centrilobular", Centrilobular),
    ("CentrilobularStructures", CentrilobularStructures),
    ("CerebroplacentalRatio", CerebroplacentalRatio),
    ("CertaintyOfFeature", CertaintyOfFeature),
    ("CertaintyOfFinding", CertaintyOfFinding),
    ("CertaintyOfImpression", CertaintyOfImpression),
    ("CessationOfExercise", CessationOfExercise),
    ("CessationOfStimulation", CessationOfStimulation),
    ("Cetuximab89Zr", Cetuximab89Zr),
    ("ChairOfProtocolCommittee", ChairOfProtocolCommittee),
    ("ChangMethod", ChangMethod),
    ("ChangeInPatientAnatomy", ChangeInPatientAnatomy),
    ("ChangeOfProcedureForCorrectCharging", ChangeOfProcedureForCorrectCharging),
    ("ChannelObstruction", ChannelObstruction),
    ("ChebyshevFilter", ChebyshevFilter),
    ("CheckedInStatus", CheckedInStatus),
    ("Checkerboard", Checkerboard),
    ("ChestCADReport", ChestCADReport),
    ("ChestTube", ChestTube),
    ("ChiSquare", ChiSquare),
    ("CholineC11", CholineC11),
    ("CholinePerCreatineRatio", CholinePerCreatineRatio),
    ("ChoriocapillarisStructuralReflectanceMap", ChoriocapillarisStructuralReflectanceMap),
    ("ChoriocapillarisVasculatureFlow", ChoriocapillarisVasculatureFlow),
    ("ChoroidStructuralReflectanceMap", ChoroidStructuralReflectanceMap),
    ("ChoroidVasculatureFlow", ChoroidVasculatureFlow),
    ("ChromaticityEvaluation", ChromaticityEvaluation),
    ("CineFilmDigitized", CineFilmDigitized),
    ("Cinefluorography", Cinefluorography),
    ("CircadianEffects", CircadianEffects),
    ("CircleBScanPattern", CircleBScanPattern),
    ("CircleRadialBScanPattern", CircleRadialBScanPattern),
    ("CircleRasterBScanPattern", CircleRasterBScanPattern),
    ("CircularMethod", CircularMethod),
    ("Circulating", Circulating),
    ("CirculatorySupport", CirculatorySupport),
    ("CircumferentialLagrangianStrain", CircumferentialLagrangianStrain),
    ("CircumferentialLagrangianStrainRate", CircumferentialLagrangianStrainRate),
    ("CircumpapillaryRetinalNerveFiberLayerKeyMeasurements", CircumpapillaryRetinalNerveFiberLayerKeyMeasurements),
    ("Circumscribed", Circumscribed),
    ("ClassActivation", ClassActivation),
    ("CleanDescriptorsOption", CleanDescriptorsOption),
    ("CleanGraphicsOption", CleanGraphicsOption),
    ("CleanPixelDataOption", CleanPixelDataOption),
    ("CleanRecognizableVisualFeaturesOption", CleanRecognizableVisualFeaturesOption),
    ("CleanStructuredContentOption", CleanStructuredContentOption),
    ("ClearanceInterlock", ClearanceInterlock),
    ("ClinicalContext", ClinicalContext),
    ("ClinicalEvaluation", ClinicalEvaluation),
    ("ClinicalFinding", ClinicalFinding),
    ("ClinicalFractionNumber", ClinicalFractionNumber),
    ("ClinicalInterpretation", ClinicalInterpretation),
    ("ClinicalTrialSubmissionInputUsed", ClinicalTrialSubmissionInputUsed),
    ("ClipboardManager", ClipboardManager),
    ("ClockfaceOrRegion", ClockfaceOrRegion),
    ("ClusterAnalysis", ClusterAnalysis),
    ("ClusterProminenceOfGLCM", ClusterProminenceOfGLCM),
    ("ClusterShadeOfGLCM", ClusterShadeOfGLCM),
    ("ClusterTendencyOfGLCM", ClusterTendencyOfGLCM),
    ("ClusteredMicrocysts", ClusteredMicrocysts),
    ("CoachingDevice", CoachingDevice),
    ("Coalescent", Coalescent),
    ("CockroftGaultFormulaEstimationOfGFR", CockroftGaultFormulaEstimationOfGFR),
    ("CoilMarker", CoilMarker),
    ("CoilPlacementConcern", CoilPlacementConcern),
    ("CoilSelectionConcern", CoilSelectionConcern),
    ("Coin", Coin),
    ("CollectionOfPresentationStates", CollectionOfPresentationStates),
    ("CollimatedFieldArea", CollimatedFieldArea),
    ("CollimatedFieldHeight", CollimatedFieldHeight),
    ("CollimatedFieldWidth", CollimatedFieldWidth),
    ("CollimationTooCloseToBreast", CollimationTooCloseToBreast),
    ("Collimator", Collimator),
    ("CollisionChecks", CollisionChecks),
    ("ColonCADReport", ColonCADReport),
    ("ColonOverallAssessment", ColonOverallAssessment),
    ("ColorFlowDoppler", ColorFlowDoppler),
    ("ColumnAngulation", ColumnAngulation),
    ("CombinationImplant", CombinationImplant),
    ("CombinedCardiacOutput", CombinedCardiacOutput),
    ("CombinedPosteriorEnhancementAndShadowing", CombinedPosteriorEnhancementAndShadowing),
    ("CombinedStructuredLightPerThermalImager", CombinedStructuredLightPerThermalImager),
    ("Comment", Comment),
    ("CompactFlash", CompactFlash),
    ("ComparativeAverage10YearCHDRisk", ComparativeAverage10YearCHDRisk),
    ("ComparativeLow10YearCHDRisk", ComparativeLow10YearCHDRisk),
    ("ComparisonToPreviousStudies", ComparisonToPreviousStudies),
    ("ComparisonWithPriorStudyDone", ComparisonWithPriorStudyDone),
    ("CompleteAcquisitionContent", CompleteAcquisitionContent),
    ("CompleteRenderingForPresentation", CompleteRenderingForPresentation),
    ("CompleteStudyContent", CompleteStudyContent),
    ("CompletelyEncapsulated", CompletelyEncapsulated),
    ("Complex", Complex),
    ("ComplexCyst", ComplexCyst),
    ("ComplicatedCyst", ComplicatedCyst),
    ("Complications", Complications),
    ("ComponentConnection", ComponentConnection),
    ("ComponentID", ComponentID),
    ("ComponentModel", ComponentModel),
    ("ComponentType", ComponentType),
    ("ComponentVolume", ComponentVolume),
    ("ComposedFromDeformableRegistration", ComposedFromDeformableRegistration),
    ("ComposedFromImageSeriesOtherThanPlanningImageSeries", ComposedFromImageSeriesOtherThanPlanningImageSeries),
    ("ComposedFromPriorDoses", ComposedFromPriorDoses),
    ("ComposedFromPriorDosesAndCurrentPlan", ComposedFromPriorDosesAndCurrentPlan),
    ("ComposedFromSetupPerturbation", ComposedFromSetupPerturbation),
    ("ComposedWithRadiobiologicalEffects", ComposedWithRadiobiologicalEffects),
    ("ComposedWithWeightingForFractionsDelivered", ComposedWithWeightingForFractionsDelivered),
    ("CompositeFeature", CompositeFeature),
    ("CompositeFeatureModifier", CompositeFeatureModifier),
    ("CompositeType", CompositeType),
    ("CompoundStatement", CompoundStatement),
    ("CompressionContactArea", CompressionContactArea),
    ("CompressionForce", CompressionForce),
    ("CompressionPressure", CompressionPressure),
    ("CompressionThickness", CompressionThickness),
    ("ComptonScattering", ComptonScattering),
    ("ComputationServer", ComputationServer),
    ("ComputedFromImageAttributes", ComputedFromImageAttributes),
    ("ComputedRadiography", ComputedRadiography),
    ("ComputedTomography", ComputedTomography),
    ("ComputerAidedDetection", ComputerAidedDetection),
    ("ComputerAidedDiagnosis", ComputerAidedDiagnosis),
    ("ComputerAssistedDetectionPerDiagnosis", ComputerAssistedDetectionPerDiagnosis),
    ("Concentration", Concentration),
    ("ConcentricCircleBScanPattern", ConcentricCircleBScanPattern),
    ("Concern", Concern),
    ("Conclusion", Conclusion),
    ("Conclusions", Conclusions),
    ("ConditionEffectiveDoseMeasured", ConditionEffectiveDoseMeasured),
    ("ConePresent", ConePresent),
    ("ConfiguredDefaultImagingEnergy", ConfiguredDefaultImagingEnergy),
    ("ConfiguredLowestImagingEnergy", ConfiguredLowestImagingEnergy),
    ("ConfirmationOfTarget", ConfirmationOfTarget),
    ("ConfocalImaging", ConfocalImaging),
    ("ConfocalMicroscopy", ConfocalMicroscopy),
    ("ConformalArcBeam", ConformalArcBeam),
    ("ConformalityShell", ConformalityShell),
    ("ConjunctiveTerm", ConjunctiveTerm),
    ("ConnectedImplantationPlanComponent", ConnectedImplantationPlanComponent),
    ("ConsistentWithLabelingOfTheDevice", ConsistentWithLabelingOfTheDevice),
    ("ConstantAngleAcquisition", ConstantAngleAcquisition),
    ("ConsultationWith", ConsultationWith),
    ("ConsumableCatheterType", ConsumableCatheterType),
    ("ConsumableIsNew", ConsumableIsNew),
    ("ConsumableReturnedToInventory", ConsumableReturnedToInventory),
    ("ConsumableTakenFromInventory", ConsumableTakenFromInventory),
    ("ConsumableUnusable", ConsumableUnusable),
    ("ContentAssessmentResult", ContentAssessmentResult),
    ("ContentDate", ContentDate),
    ("ContentTime", ContentTime),
    ("ContinentalKennelClub", ContinentalKennelClub),
    ("ContinuityEquation", ContinuityEquation),
    ("ContinuityEquationByMeanVelocity", ContinuityEquationByMeanVelocity),
    ("ContinuityEquationByPeakVelocity", ContinuityEquationByPeakVelocity),
    ("ContinuityEquationByVelocityTimeIntegral", ContinuityEquationByVelocityTimeIntegral),
    ("Continuous", Continuous),
    ("ContinuousPermanent", ContinuousPermanent),
    ("ContinuousTemporary", ContinuousTemporary),
    ("ContourEnd", ContourEnd),
    ("ContourRealignment", ContourRealignment),
    ("ContourStart", ContourStart),
    ("ContouringInputUsed", ContouringInputUsed),
    ("ContrastAdministered", ContrastAdministered),
    ("ContrastAgentAngioMRSignalIntensity", ContrastAgentAngioMRSignalIntensity),
    ("ContrastEnd", ContrastEnd),
    ("ContrastLongitudinalRelaxivity", ContrastLongitudinalRelaxivity),
    ("ContrastOfGLCM", ContrastOfGLCM),
    ("ContrastPerBolusAgent", ContrastPerBolusAgent),
    ("ContrastPerBolusIngredientOpaque", ContrastPerBolusIngredientOpaque),
    ("ContrastPhase", ContrastPhase),
    ("ContrastReactionProphylacticAgent", ContrastReactionProphylacticAgent),
    ("ContrastStart", ContrastStart),
    ("ContrastTransverseRelaxivity", ContrastTransverseRelaxivity),
    ("ContrastVolumeLimit", ContrastVolumeLimit),
    ("ControlledSSDSetupMethod", ControlledSSDSetupMethod),
    ("ConversionFactor", ConversionFactor),
    ("ConvolutionSuperposition", ConvolutionSuperposition),
    ("CoopersLigamentChanges", CoopersLigamentChanges),
    ("CoordinateAlignmentImageSeries", CoordinateAlignmentImageSeries),
    ("CopiedFromImageAttributes", CopiedFromImageAttributes),
    ("CornCobBedding", CornCobBedding),
    ("CornealAxialPowerMap", CornealAxialPowerMap),
    ("CornealBirefringenceCompensation", CornealBirefringenceCompensation),
    ("CornealElevationMap", CornealElevationMap),
    ("CornealInstantaneousPowerMap", CornealInstantaneousPowerMap),
    ("CornealRefractivePowerMap", CornealRefractivePowerMap),
    ("CornealWavefrontMap", CornealWavefrontMap),
    ("CoronaRadiata", CoronaRadiata),
    ("CoronaryArteryBypassDuringThisAdmission", CoronaryArteryBypassDuringThisAdmission),
    ("CoronaryArteryDiseaseAssessment", CoronaryArteryDiseaseAssessment),
    ("CoronaryArteryDiseaseAssessmentModifier", CoronaryArteryDiseaseAssessmentModifier),
    ("CoronaryArteryDiseaseStenosisAssessmentModifier", CoronaryArteryDiseaseStenosisAssessmentModifier),
    ("CoronaryFlowReserve", CoronaryFlowReserve),
    ("CoronaryLesionGreaterThanEquals50PercentStenosis", CoronaryLesionGreaterThanEquals50PercentStenosis),
    ("CorrectedSinusNodeRecoveryTime", CorrectedSinusNodeRecoveryTime),
    ("CorrectionFactor", CorrectionFactor),
    ("CorrectionOfCongenitalCardiovascularDeformity", CorrectionOfCongenitalCardiovascularDeformity),
    ("CorrelatesToMRIFindings", CorrelatesToMRIFindings),
    ("CorrelatesToMammographyFindings", CorrelatesToMammographyFindings),
    ("CorrelatesToOtherImagingFindings", CorrelatesToOtherImagingFindings),
    ("CorrelatesToPhysicalExaminationFindings", CorrelatesToPhysicalExaminationFindings),
    ("CorrelatesToUltrasoundFindings", CorrelatesToUltrasoundFindings),
    ("CorrelationOfGLCM", CorrelationOfGLCM),
    ("CorrelationToOtherFindings", CorrelationToOtherFindings),
    ("CouchIndexLabel", CouchIndexLabel),
    ("CountryOfLanguage", CountryOfLanguage),
    ("CoverSlipDefect", CoverSlipDefect),
    ("CoverslipEdge", CoverslipEdge),
    ("CreatineAndCholine", CreatineAndCholine),
    ("CreatinePlusCholinePerCitrateRatio", CreatinePlusCholinePerCitrateRatio),
    ("CrossBScanPattern", CrossBScanPattern),
    ("CrossSectionalScanPlaneOrientation", CrossSectionalScanPlaneOrientation),
    ("CrossedGrid", CrossedGrid),
    ("Crosstable", Crosstable),
    ("CubeBScanPattern", CubeBScanPattern),
    ("CubeMethod", CubeMethod),
    ("Culposcopy", Culposcopy),
    ("CupToDiscAreaRatio", CupToDiscAreaRatio),
    ("CupToDiscRatioHorizontal", CupToDiscRatioHorizontal),
    ("CupToDiscRatioVertical", CupToDiscRatioVertical),
    ("CurrentFractionNumber", CurrentFractionNumber),
    ("CurrentPregnancyKnownOrSuspectedMalformationsPerSyndromes", CurrentPregnancyKnownOrSuspectedMalformationsPerSyndromes),
    ("CurrentProcedureDescriptions", CurrentProcedureDescriptions),
    ("CurrentProcedureEvidence", CurrentProcedureEvidence),
    ("CurrentResponse", CurrentResponse),
    ("CurveFitMethod", CurveFitMethod),
    ("CurveFitParameter", CurveFitParameter),
    ("CurveFittedReference", CurveFittedReference),
    ("CurvedLinearUltrasoundTransducerGeometry", CurvedLinearUltrasoundTransducerGeometry),
    ("CurvedMultiplanarReformatting", CurvedMultiplanarReformatting),
    ("CutOffTissue", CutOffTissue),
    ("CylinderMarker", CylinderMarker),
    ("CystFill", CystFill),
    ("Cystoscopy", Cystoscopy),
    ("CytologicAnalysis", CytologicAnalysis),
    ("DICOMInstancesAccessed", DICOMInstancesAccessed),
    ("DICOMInstancesTransferred", DICOMInstancesTransferred),
    ("DICOMObjectCatalog", DICOMObjectCatalog),
    ("DICOMStudyDeleted", DICOMStudyDeleted),
    ("DINGeometryPattern", DINGeometryPattern),
    ("DINGrayscalePattern", DINGrayscalePattern),
    ("DINResolutionPattern", DINResolutionPattern),
    ("DKI", DKI),
    ("DLP", DLP),
    ("DLPAlertValue", DLPAlertValue),
    ("DLPAlertValueConfigured", DLPAlertValueConfigured),
    ("DLPForwardEstimate", DLPForwardEstimate),
    ("DLPFrontProjectionSystem", DLPFrontProjectionSystem),
    ("DLPNotificationValue", DLPNotificationValue),
    ("DLPNotificationValueConfigured", DLPNotificationValueConfigured),
    ("DLPRearProjectionSystem", DLPRearProjectionSystem),
    ("DLPToEConversionViaMCComputation", DLPToEConversionViaMCComputation),
    ("DLPToEConversionViaMeasurement", DLPToEConversionViaMeasurement),
    ("DN3089Zr", DN3089Zr),
    ("DOT", DOT),
    ("DPA71311C", DPA71311C),
    ("DPA71418F", DPA71418F),
    ("DSI", DSI),
    ("DTI", DTI),
    ("DVAWavePeakVelocity", DVAWavePeakVelocity),
    ("DVD", DVD),
    ("DVDImported", DVDImported),
    ("DVPeakVelocityIndexInVeins", DVPeakVelocityIndexInVeins),
    ("DVPreloadIndex", DVPreloadIndex),
    ("DVPulsatilityIndexInVeins", DVPulsatilityIndexInVeins),
    ("DVSPerA", DVSPerA),
    ("DVSWavePeakVelocity", DVSWavePeakVelocity),
    ("DW", DW),
    ("DarkBrownColor", DarkBrownColor),
    ("DarkRedColor", DarkRedColor),
    ("DarkfieldIllumination", DarkfieldIllumination),
    ("DataNotAccessibleFromStorage", DataNotAccessibleFromStorage),
    ("DataRetentionPolicyExpired", DataRetentionPolicyExpired),
    ("DateStickerIsMissing", DateStickerIsMissing),
    ("DateTimeConcernNoted", DateTimeConcernNoted),
    ("DateTimeConcernResolved", DateTimeConcernResolved),
    ("DateTimeEnded", DateTimeEnded),
    ("DateTimeEstimated", DateTimeEstimated),
    ("DateTimeOfDeath", DateTimeOfDeath),
    ("DateTimeOfIntervention", DateTimeOfIntervention),
    ("DateTimeOfLastEvaluation", DateTimeOfLastEvaluation),
    ("DateTimeOfProcessing", DateTimeOfProcessing),
    ("DateTimeOfRecordingOfLogEntry", DateTimeOfRecordingOfLogEntry),
    ("DateTimeProblemObserved", DateTimeProblemObserved),
    ("DateTimeProblemResolved", DateTimeProblemResolved),
    ("DateTimeStarted", DateTimeStarted),
    ("DateTimeUnsynchronized", DateTimeUnsynchronized),
    ("DeIdentifyingEquipment", DeIdentifyingEquipment),
    ("DeathDuringCatheterization", DeathDuringCatheterization),
    ("DeathDuringThisAdmission", DeathDuringThisAdmission),
    ("DeepCapillaryPlexusFlow", DeepCapillaryPlexusFlow),
    ("DeepCapillaryPlexusMap", DeepCapillaryPlexusMap),
    ("DeepRetinaStructuralReflectanceMap", DeepRetinaStructuralReflectanceMap),
    ("DeepRetinaVasculatureFlow", DeepRetinaVasculatureFlow),
    ("DeformableModels", DeformableModels),
    ("DeformedForRegistration", DeformedForRegistration),
    ("DegreeOfFreedomExactRotationalValue", DegreeOfFreedomExactRotationalValue),
    ("DegreeOfFreedomExactTranslationalValue", DegreeOfFreedomExactTranslationalValue),
    ("DegreeOfFreedomID", DegreeOfFreedomID),
    ("DegreeOfFreedomMaximumRotationalValue", DegreeOfFreedomMaximumRotationalValue),
    ("DegreeOfFreedomMaximumTranslationalValue", DegreeOfFreedomMaximumTranslationalValue),
    ("DegreeOfFreedomMinimumRotationalValue", DegreeOfFreedomMinimumRotationalValue),
    ("DegreeOfFreedomMinimumTranslationalValue", DegreeOfFreedomMinimumTranslationalValue),
    ("DegreeOfThrombus", DegreeOfThrombus),
    ("DegreesOfFreedomSpecification", DegreesOfFreedomSpecification),
    ("DelayExportUntilClinicalInformationIsAvailable", DelayExportUntilClinicalInformationIsAvailable),
    ("DelayExportUntilConfirmationOfDiagnosisIsAvailable", DelayExportUntilConfirmationOfDiagnosisIsAvailable),
    ("DelayExportUntilExpertReviewIsAvailable", DelayExportUntilExpertReviewIsAvailable),
    ("DelayExportUntilFinalReportIsAvailable", DelayExportUntilFinalReportIsAvailable),
    ("DelayExportUntilHistopathologyIsAvailable", DelayExportUntilHistopathologyIsAvailable),
    ("DelayExportUntilOtherLaboratoryResultsAreAvailable", DelayExportUntilOtherLaboratoryResultsAreAvailable),
    ("DelayExportUntilPatientDies", DelayExportUntilPatientDies),
    ("DelayExportUntilPatientIsDischarged", DelayExportUntilPatientIsDischarged),
    ("DelayedRedistributionState", DelayedRedistributionState),
    ("DeliveryRateToleranceViolation", DeliveryRateToleranceViolation),
    ("DeliveryVerificationNote", DeliveryVerificationNote),
    ("DensitometricLuminalCrossSectionalAreaGraph", DensitometricLuminalCrossSectionalAreaGraph),
    ("DensitometricMethod", DensitometricMethod),
    ("Density", Density),
    ("Density_111103", Density_111103),
    ("DepartmentSystemScheduler", DepartmentSystemScheduler),
    ("DependentOpacity", DependentOpacity),
    ("Deployment", Deployment),
    ("DeprecatedProtocol", DeprecatedProtocol),
    ("Depth", Depth),
    ("Derivation", Derivation),
    ("DerivedDose", DerivedDose),
    ("DerivedEffectiveDiameter", DerivedEffectiveDiameter),
    ("DerivedFiducial", DerivedFiducial),
    ("DerivedFromHumanReadableReports", DerivedFromHumanReadableReports),
    ("DerivedHemodynamicMeasurements", DerivedHemodynamicMeasurements),
    ("DerivedImagingMeasurements", DerivedImagingMeasurements),
    ("DerivedNonValveArea", DerivedNonValveArea),
    ("DerivedPlanningData", DerivedPlanningData),
    ("DerivedPlanningImages", DerivedPlanningImages),
    ("Dermoscopy", Dermoscopy),
    ("DescriptionOfChange", DescriptionOfChange),
    ("DescriptionOfMaterial", DescriptionOfMaterial),
    ("Designator", Designator),
    ("DestinationMedia", DestinationMedia),
    ("DestinationRoleID", DestinationRoleID),
    ("DestructionOfMicrobubbles", DestructionOfMicrobubbles),
    ("Detach", Detach),
    ("DetectionPerformed", DetectionPerformed),
    ("DetectorArtifactS", DetectorArtifactS),
    ("DetectorType", DetectorType),
    ("Deterministic", Deterministic),
    ("DeviationIndex", DeviationIndex),
    ("Device", Device),
    ("DeviceCode", DeviceCode),
    ("DeviceCrossedLesion", DeviceCrossedLesion),
    ("DeviceDeployed", DeviceDeployed),
    ("DeviceDescription", DeviceDescription),
    ("DeviceLocalizer", DeviceLocalizer),
    ("DeviceManufacturer", DeviceManufacturer),
    ("DeviceModelName", DeviceModelName),
    ("DeviceName", DeviceName),
    ("DeviceObserverManufacturer", DeviceObserverManufacturer),
    ("DeviceObserverManufacturerClassUID", DeviceObserverManufacturerClassUID),
    ("DeviceObserverModelName", DeviceObserverModelName),
    ("DeviceObserverName", DeviceObserverName),
    ("DeviceObserverPhysicalLocationDuringObservation", DeviceObserverPhysicalLocationDuringObservation),
    ("DeviceObserverSerialNumber", DeviceObserverSerialNumber),
    ("DeviceObserverUID", DeviceObserverUID),
    ("DeviceRelatedAndEnvironmentRelatedEvent", DeviceRelatedAndEnvironmentRelatedEvent),
    ("DeviceRoleInProcedure", DeviceRoleInProcedure),
    ("DeviceSerialNumber", DeviceSerialNumber),
    ("DeviceSubject", DeviceSubject),
    ("DeviceSubjectManufacturer", DeviceSubjectManufacturer),
    ("DeviceSubjectModelName", DeviceSubjectModelName),
    ("DeviceSubjectName", DeviceSubjectName),
    ("DeviceSubjectPhysicalLocationDuringObservation", DeviceSubjectPhysicalLocationDuringObservation),
    ("DeviceSubjectSerialNumber", DeviceSubjectSerialNumber),
    ("DeviceSubjectUID", DeviceSubjectUID),
    ("DfCD4589Zr", DfCD4589Zr),
    ("DfFK23PEG489Zr", DfFK23PEG489Zr),
    ("DfFK289Zr", DfFK289Zr),
    ("DfFK89Zr", DfFK89Zr),
    ("DfFKPEG389Zr", DfFKPEG389Zr),
    ("DgN", DgN),
    ("DiabeticFibrousMastopathy", DiabeticFibrousMastopathy),
    ("DiagnosisInputUsed", DiagnosisInputUsed),
    ("DiagnosticAdministration", DiagnosticAdministration),
    ("DiagnosticSourceImages", DiagnosticSourceImages),
    ("DiameterBasedMethod", DiameterBasedMethod),
    ("DiameterGraph", DiameterGraph),
    ("DiameterRangeMaximumValue", DiameterRangeMaximumValue),
    ("DiameterRangeMinimumValue", DiameterRangeMinimumValue),
    ("DiameterSymmetry", DiameterSymmetry),
    ("DiamondDetector", DiamondDetector),
    ("Diaphanography", Diaphanography),
    ("DiaphragmaticAttenuation", DiaphragmaticAttenuation),
    ("DiastolicBloodVelocityMean", DiastolicBloodVelocityMean),
    ("DiastolicBloodVelocityPeak", DiastolicBloodVelocityPeak),
    ("DiastolicPerSystolicVelocityRatio", DiastolicPerSystolicVelocityRatio),
    ("DiastolicPressureAverage", DiastolicPressureAverage),
    ("DiastolicPressureNadir", DiastolicPressureNadir),
    ("DiceSimilarityCoefficient", DiceSimilarityCoefficient),
    ("Dictation", Dictation),
    ("DifferenceAverageOfGLCM", DifferenceAverageOfGLCM),
    ("DifferenceEntropyOfGLCM", DifferenceEntropyOfGLCM),
    ("DifferenceInBorderDefinition", DifferenceInBorderDefinition),
    ("DifferenceInBorderShape", DifferenceInBorderShape),
    ("DifferenceInDistribution", DifferenceInDistribution),
    ("DifferenceInEDLVVolume", DifferenceInEDLVVolume),
    ("DifferenceInEjectionFraction", DifferenceInEjectionFraction),
    ("DifferenceInSiteInvolvement", DifferenceInSiteInvolvement),
    ("DifferenceInTexture", DifferenceInTexture),
    ("DifferenceInTypeOfContent", DifferenceInTypeOfContent),
    ("DifferenceVarianceOfGLCM", DifferenceVarianceOfGLCM),
    ("DifferentialDiagnosisPerImpression", DifferentialDiagnosisPerImpression),
    ("DifferentialInterferenceContrast", DifferentialInterferenceContrast),
    ("DifferentialSignal", DifferentialSignal),
    ("DifficultPhysicalPerClinicalExamination", DifficultPhysicalPerClinicalExamination),
    ("DiffuseContrastEnhancement", DiffuseContrastEnhancement),
    ("DiffuseDirectIllumination", DiffuseDirectIllumination),
    ("DiffuselyIncreasedVascularityInSurroundingTissue", DiffuselyIncreasedVascularityInSurroundingTissue),
    ("DiffusionAnisotropy", DiffusionAnisotropy),
    ("DiffusionAttenuated", DiffusionAttenuated),
    ("DiffusionCoefficient", DiffusionCoefficient),
    ("DiffusionImageAnalysis", DiffusionImageAnalysis),
    ("DiffusionTractography", DiffusionTractography),
    ("DiffusionWeighted", DiffusionWeighted),
    ("DiffusionWeightedAcquisitionHighestBValueImage", DiffusionWeightedAcquisitionHighestBValueImage),
    ("DigitalFluoroscopy", DigitalFluoroscopy),
    ("DigitalMammography", DigitalMammography),
    ("DigitalMicroscopy", DigitalMicroscopy),
    ("DigitalRadiography", DigitalRadiography),
    ("DigitalSubtractionAngiography", DigitalSubtractionAngiography),
    ("DigitalTimecodeNOS", DigitalTimecodeNOS),
    ("Diode", Diode),
    ("DiodeArray", DiodeArray),
    ("DirectDetector", DirectDetector),
    ("DirectlyMeasured", DirectlyMeasured),
    ("DisagreementWithPriorFindings", DisagreementWithPriorFindings),
    ("DisapprovedForAnyUse", DisapprovedForAnyUse),
    ("DisapprovedForExperimentalUse", DisapprovedForExperimentalUse),
    ("DisapprovedForUseAtTheInstitution", DisapprovedForUseAtTheInstitution),
    ("DisapprovedForUseInTheClinicalTrial", DisapprovedForUseInTheClinicalTrial),
    ("DisapprovedForUseOnPregnantPatients", DisapprovedForUseOnPregnantPatients),
    ("DiscCentered", DiscCentered),
    ("DiscFovea", DiscFovea),
    ("DiscMaculaCentered", DiscMaculaCentered),
    ("Discarded", Discarded),
    ("DischargeDateTime", DischargeDateTime),
    ("DischargeSummary", DischargeSummary),
    ("DiscontinuedForUnspecifiedReason", DiscontinuedForUnspecifiedReason),
    ("DiscontinuedProcedureStepRescheduled", DiscontinuedProcedureStepRescheduled),
    ("DiscontinuedProcedureStepReschedulingRecommended", DiscontinuedProcedureStepReschedulingRecommended),
    ("DissectionClassification", DissectionClassification),
    ("DissectionWithEntireSpecimenSubmission", DissectionWithEntireSpecimenSubmission),
    ("DissectionWithRepresentativeSectionsSubmission", DissectionWithRepresentativeSectionsSubmission),
    ("DissimilarityOfGLCM", DissimilarityOfGLCM),
    ("DistalCentralizer", DistalCentralizer),
    ("DistalFindingSite", DistalFindingSite),
    ("DistalReference", DistalReference),
    ("DistalStentMargin", DistalStentMargin),
    ("Distance", Distance),
    ("DistanceCorrection", DistanceCorrection),
    ("DistanceFromAnus", DistanceFromAnus),
    ("DistanceFromChestWall", DistanceFromChestWall),
    ("DistanceFromNeurovascularBundle", DistanceFromNeurovascularBundle),
    ("DistanceFromNipple", DistanceFromNipple),
    ("DistanceFromSkin", DistanceFromSkin),
    ("DistancePupillaryDistance", DistancePupillaryDistance),
    ("DistanceSourceToDetector", DistanceSourceToDetector),
    ("DistanceSourceToIsocenter", DistanceSourceToIsocenter),
    ("DistanceSourceToReferencePoint", DistanceSourceToReferencePoint),
    ("DistanceSourceToTablePlane", DistanceSourceToTablePlane),
    ("DistinctlyDefined", DistinctlyDefined),
    ("DistortedImplant", DistortedImplant),
    ("DistortionArtifactInTheAreaOfInterest", DistortionArtifactInTheAreaOfInterest),
    ("DistributedDiffusionCoefficient", DistributedDiffusionCoefficient),
    ("DistributionDescriptor", DistributionDescriptor),
    ("DistributionRepresentation", DistributionRepresentation),
    ("DivideByZero", DivideByZero),
    ("DoctorCanceledProcedure", DoctorCanceledProcedure),
    ("DocumentDigitizerEquipment", DocumentDigitizerEquipment),
    ("DocumentTitle", DocumentTitle),
    ("DocumentTitleModifier", DocumentTitleModifier),
    ("Document_121003", Document_121003),
    ("Document_DOC", Document_DOC),
    ("DogRegistryOfAmerica", DogRegistryOfAmerica),
    ("DominantFollicle", DominantFollicle),
    ("DoorInterlock", DoorInterlock),
    ("DopplerAngle", DopplerAngle),
    ("DopplerAudio", DopplerAudio),
    ("DopplerOCTA", DopplerOCTA),
    ("DopplerVolumeFlow", DopplerVolumeFlow),
    ("DoseAreaProduct", DoseAreaProduct),
    ("DoseAreaProductTotal", DoseAreaProductTotal),
    ("DoseCalculationBoundingVolume", DoseCalculationBoundingVolume),
    ("DoseCalculationImageSeries", DoseCalculationImageSeries),
    ("DoseCalibrator", DoseCalibrator),
    ("DoseCheckAlertDetails", DoseCheckAlertDetails),
    ("DoseCheckNotificationDetails", DoseCheckNotificationDetails),
    ("DoseFrequency", DoseFrequency),
    ("DoseFromPriorTreatment", DoseFromPriorTreatment),
    ("DoseGradient", DoseGradient),
    ("DoseImage", DoseImage),
    ("DoseLimitInterlock", DoseLimitInterlock),
    ("DoseMeasurementDevice", DoseMeasurementDevice),
    ("DosePointCloud", DosePointCloud),
    ("DoseRP", DoseRP),
    ("DoseRPTotal", DoseRPTotal),
    ("DoseRate", DoseRate),
    ("DoseRateToleranceViolation", DoseRateToleranceViolation),
    ("DosimeterType", DosimeterType),
    ("DosimetricResult", DosimetricResult),
    ("DosingFactor", DosingFactor),
    ("DoubleExposure", DoubleExposure),
    ("DoubleMDerivedFilter", DoubleMDerivedFilter),
    ("DoubleProduct", DoubleProduct),
    ("DrugAdministered", DrugAdministered),
    ("DrugEnd", DrugEnd),
    ("DrugProductIdentifier", DrugProductIdentifier),
    ("DrugStart", DrugStart),
    ("DualSideIllumination", DualSideIllumination),
    ("DualSpeedOfSoundCorrection", DualSpeedOfSoundCorrection),
    ("DuctNarrowing", DuctNarrowing),
    ("DuctalAdenoma", DuctalAdenoma),
    ("DuctalHyperplasia", DuctalHyperplasia),
    ("Ductography", Ductography),
    ("DuctusArteriosusArch", DuctusArteriosusArch),
    ("DuplexDoppler", DuplexDoppler),
    ("DuplicateOrder", DuplicateOrder),
    ("DurationOfAdministration", DurationOfAdministration),
    ("DurationOfIntervention", DurationOfIntervention),
    ("DurationOfTimePeriod", DurationOfTimePeriod),
    ("DuringTransport", DuringTransport),
    ("DuringVoiding", DuringVoiding),
    ("DwConversionFactorCoefficients", DwConversionFactorCoefficients),
    ("DynamicContrastEnhancedAcquisition", DynamicContrastEnhancedAcquisition),
    ("DynamicContrastEnhancedAcquisitionSubtractionImage", DynamicContrastEnhancedAcquisitionSubtractionImage),
    ("DynamicContrastEnhancedTemporalResolution", DynamicContrastEnhancedTemporalResolution),
    ("DynamicContrastMRMeasurementReport", DynamicContrastMRMeasurementReport),
    ("DynamicLight", DynamicLight),
    ("DynamicWedge", DynamicWedge),
    ("DyssynchronyIndex", DyssynchronyIndex),
    ("E4G1089Zr", E4G1089Zr),
    ("ECGAnnotation", ECGAnnotation),
    ("ECGBasedGatingSignalProcessed", ECGBasedGatingSignalProcessed),
    ("ECGBasedModulation", ECGBasedModulation),
    ("ECGGlobalMeasurements", ECGGlobalMeasurements),
    ("ECGLeadMeasurements", ECGLeadMeasurements),
    ("ECST", ECST),
    ("EEGAnnotation", EEGAnnotation),
    ("EEMCrossSectionalArea", EEMCrossSectionalArea),
    ("EEMDiameter", EEMDiameter),
    ("EEMDiameterRatio", EEMDiameterRatio),
    ("EEMVolume", EEMVolume),
    ("EMGAnnotation", EMGAnnotation),
    ("EOGAnnotation", EOGAnnotation),
    ("EPProcedurePhase", EPProcedurePhase),
    ("ESUR2012ProstateMRIAcquisitionRequirements", ESUR2012ProstateMRIAcquisitionRequirements),
    ("EVO", EVO),
    ("EarlyContrastEnhancement", EarlyContrastEnhancement),
    ("EarlyGestation", EarlyGestation),
    ("EchoPattern", EchoPattern),
    ("Echocardiography", Echocardiography),
    ("EchogenicHalo", EchogenicHalo),
    ("Echogenicity", Echogenicity),
    ("Echovist", Echovist),
    ("Ecromeximab89Zr", Ecromeximab89Zr),
    ("EdgeDetection", EdgeDetection),
    ("EdgeEnhancement", EdgeEnhancement),
    ("EditedModel", EditedModel),
    ("EducationalIntent", EducationalIntent),
    ("EffectiveAtomicNumber", EffectiveAtomicNumber),
    ("EffectiveDose", EffectiveDose),
    ("EffectiveDoseConversionFactor", EffectiveDoseConversionFactor),
    ("EffectiveDosePhantomType", EffectiveDosePhantomType),
    ("EffectiveOrificeArea", EffectiveOrificeArea),
    ("EffectiveZ", EffectiveZ),
    ("EffectiveZPerA", EffectiveZPerA),
    ("Elasticity", Elasticity),
    ("ElectricalFailure", ElectricalFailure),
    ("Electrocardiography", Electrocardiography),
    ("ElectrodePlacement", ElectrodePlacement),
    ("Electroencephalography", Electroencephalography),
    ("ElectromechanicalDelay", ElectromechanicalDelay),
    ("Electromyography", Electromyography),
    ("ElectronFixedAperture", ElectronFixedAperture),
    ("Electrooculography", Electrooculography),
    ("ElementalCompositionAtomicMassFraction", ElementalCompositionAtomicMassFraction),
    ("ElevationBasedCornealTomographer", ElevationBasedCornealTomographer),
    ("Eligibility", Eligibility),
    ("EligibilityReader", EligibilityReader),
    ("EligibleForReimbursement", EligibleForReimbursement),
    ("EligibleForReimbursementOnPerPatientBasis", EligibleForReimbursementOnPerPatientBasis),
    ("Elliptic", Elliptic),
    ("EllipticFilter", EllipticFilter),
    ("EllipticalAnnulusSectors", EllipticalAnnulusSectors),
    ("Email", Email),
    ("EmailMessage", EmailMessage),
    ("EmbryonicKidney", EmbryonicKidney),
    ("EmergencyOverrideStarted", EmergencyOverrideStarted),
    ("EmergencyOverrideStopped", EmergencyOverrideStopped),
    ("EmpiricalAlgorithm", EmpiricalAlgorithm),
    ("EndCirculatorySupport", EndCirculatorySupport),
    ("EndDiastole", EndDiastole),
    ("EndOfExpiration", EndOfExpiration),
    ("EndOfInspiration", EndOfInspiration),
    ("EndOfSystole", EndOfSystole),
    ("EndOfTimePeriod", EndOfTimePeriod),
    ("EndOfXRayIrradiation", EndOfXRayIrradiation),
    ("EndOxygenAdministration", EndOxygenAdministration),
    ("EndPacing", EndPacing),
    ("EndProcedureAction", EndProcedureAction),
    ("EndVentilation", EndVentilation),
    ("EndingFlowRateOfAdministration", EndingFlowRateOfAdministration),
    ("EndocardialMethod", EndocardialMethod),
    ("EndocavitaryTransducer", EndocavitaryTransducer),
    ("Endoleak", Endoleak),
    ("EndorectalCoilType", EndorectalCoilType),
    ("EndorectalCoilUsed", EndorectalCoilUsed),
    ("EndorectalTransducer", EndorectalTransducer),
    ("Endoscopy", Endoscopy),
    ("EndothelialCellCountKeyMeasurements", EndothelialCellCountKeyMeasurements),
    ("EndothelialCellDensity", EndothelialCellDensity),
    ("EndovaginalTransducer", EndovaginalTransducer),
    ("EnhancedMultiFrameConversionEquipment", EnhancedMultiFrameConversionEquipment),
    ("EnrichmentManufacturer", EnrichmentManufacturer),
    ("EnrichmentMaterial", EnrichmentMaterial),
    ("EnrichmentMaterialPresent", EnrichmentMaterialPresent),
    ("EntireBodyTargetVolume", EntireBodyTargetVolume),
    ("EntirePullback", EntirePullback),
    ("EntranceExposureAtRP", EntranceExposureAtRP),
    ("EnvironmentalExposureHistory", EnvironmentalExposureHistory),
    ("EnvironmentalFactor", EnvironmentalFactor),
    ("EpicardialMethod", EpicardialMethod),
    ("EpidermalInclusionCyst", EpidermalInclusionCyst),
    ("EpifluorescenceIllumination", EpifluorescenceIllumination),
    ("Equation", Equation),
    ("EquationCitation", EquationCitation),
    ("EquidistantMethod", EquidistantMethod),
    ("EquipmentBroughtToProcedureRoom", EquipmentBroughtToProcedureRoom),
    ("EquipmentChange", EquipmentChange),
    ("EquipmentFailure", EquipmentFailure),
    ("EquipmentFailure_122046", EquipmentFailure_122046),
    ("EquipmentIdentification", EquipmentIdentification),
    ("EquipmentLandmark", EquipmentLandmark),
    ("EquipmentLandmarkToPatientFiducialZDistance", EquipmentLandmarkToPatientFiducialZDistance),
    ("EquipmentLandmarkXPosition", EquipmentLandmarkXPosition),
    ("EquipmentLandmarkZPosition", EquipmentLandmarkZPosition),
    ("EquipmentOrigin", EquipmentOrigin),
    ("EquipmentReady", EquipmentReady),
    ("EquipmentRemoved", EquipmentRemoved),
    ("Equivalent2GrayFractionsModel", Equivalent2GrayFractionsModel),
    ("EquivalentAttenuatorMaterial", EquivalentAttenuatorMaterial),
    ("EquivalentAttenuatorThickness", EquivalentAttenuatorThickness),
    ("EquivalentCDADocument", EquivalentCDADocument),
    ("EquivalentDose", EquivalentDose),
    ("EquivalentKReading", EquivalentKReading),
    ("EquivalentMeaningOfConceptName", EquivalentMeaningOfConceptName),
    ("EquivalentMeaningOfValue", EquivalentMeaningOfValue),
    ("ErgometerPower", ErgometerPower),
    ("ErodedPlaque", ErodedPlaque),
    ("EsophagealPressure", EsophagealPressure),
    ("Estimated", Estimated),
    ("EstimatedExtravasationActivity", EstimatedExtravasationActivity),
    ("EstimatedExtravasationVolume", EstimatedExtravasationVolume),
    ("EstimatedFromWaterEquivalentDiameter", EstimatedFromWaterEquivalentDiameter),
    ("EstimatedNormalFlow", EstimatedNormalFlow),
    ("EstimatedTimeframe", EstimatedTimeframe),
    ("EstrogenReceptor", EstrogenReceptor),
    ("Euler", Euler),
    ("EuropiumOrEuropiumCompound", EuropiumOrEuropiumCompound),
    ("EventButtonPressed", EventButtonPressed),
    ("EventButtonTest", EventButtonTest),
    ("EventUIDUsed", EventUIDUsed),
    ("ExcellentImageQuality", ExcellentImageQuality),
    ("ExcisedTissue", ExcisedTissue),
    ("ExcursionDistance", ExcursionDistance),
    ("ExerciseDurationAfterStressAgentInjection", ExerciseDurationAfterStressAgentInjection),
    ("ExerciserDevicePresent", ExerciserDevicePresent),
    ("ExogenousSubstance", ExogenousSubstance),
    ("ExpectedADCMapIsMissing", ExpectedADCMapIsMissing),
    ("ExpectedBValueIsMissing", ExpectedBValueIsMissing),
    ("Explantation", Explantation),
    ("Export", Export),
    ("ExposedRange", ExposedRange),
    ("Exposure", Exposure),
    ("ExposureIndex", ExposureIndex),
    ("ExposureTime", ExposureTime),
    ("ExposureTimePerRotation", ExposureTimePerRotation),
    ("ExposureTime_113735", ExposureTime_113735),
    ("ExtendedPatientAnatomyModel", ExtendedPatientAnatomyModel),
    ("ExtendedRenderingForPresentation", ExtendedRenderingForPresentation),
    ("ExtendedToftsModel", ExtendedToftsModel),
    ("ExternalBodyModel", ExternalBodyModel),
    ("ExternalCameraPhotography", ExternalCameraPhotography),
    ("ExternalDataSource", ExternalDataSource),
    ("ExternalTransducer", ExternalTransducer),
    ("ExtraAbdominalDesmoid", ExtraAbdominalDesmoid),
    ("ExtraProstaticFinding", ExtraProstaticFinding),
    ("ExtractionOfIndividualSubjectFromGroup", ExtractionOfIndividualSubjectFromGroup),
    ("Extravasation", Extravasation),
    ("ExtravasationVisibleInImage", ExtravasationVisibleInImage),
    ("ExtremelySmall", ExtremelySmall),
    ("FACT", FACT),
    ("FIRFilter", FIRFilter),
    ("FLB457C11", FLB457C11),
    ("FWPByGACampbell1991", FWPByGACampbell1991),
    ("FWPByGAHadlock1991", FWPByGAHadlock1991),
    ("Failed", Failed),
    ("FailedAnalyses", FailedAnalyses),
    ("FailedDetections", FailedDetections),
    ("FallyprideC11", FallyprideC11),
    ("FallyprideF18", FallyprideF18),
    ("FalseLumen", FalseLumen),
    ("FalseMarkersPerCase", FalseMarkersPerCase),
    ("FalseMarkersPerImage", FalseMarkersPerImage),
    ("FalsePositiveDefectFinding", FalsePositiveDefectFinding),
    ("FamilyHistoryFetalMalformationPerSyndrome", FamilyHistoryFetalMalformationPerSyndrome),
    ("FamilyHistoryOfMelanomaInSitu", FamilyHistoryOfMelanomaInSitu),
    ("FamilyHistoryOfNonMelanomaSkinCancer", FamilyHistoryOfNonMelanomaSkinCancer),
    ("FamilyHistoryOfProstateCancer", FamilyHistoryOfProstateCancer),
    ("FamilyHistoryUnknown", FamilyHistoryUnknown),
    ("FamilyMemberWithRiskFactor", FamilyMemberWithRiskFactor),
    ("FastDiffusionCoefficient", FastDiffusionCoefficient),
    ("FastDiffusionCoefficientFraction", FastDiffusionCoefficientFraction),
    ("FastMode", FastMode),
    ("FastingDuration", FastingDuration),
    ("FatFraction", FatFraction),
    ("FeatureDetectedOnImagesFromMultipleModalities", FeatureDetectedOnImagesFromMultipleModalities),
    ("FeatureDetectedOnMultipleImages", FeatureDetectedOnMultipleImages),
    ("FeatureDetectedOnOnlyOneOfTheImages", FeatureDetectedOnOnlyOneOfTheImages),
    ("FeatureDetectedOnTheOnlyImage", FeatureDetectedOnTheOnlyImage),
    ("FederationOfInternationalCanines", FederationOfInternationalCanines),
    ("FeedManufacturer", FeedManufacturer),
    ("FeedProductCode", FeedProductCode),
    ("FeedProductName", FeedProductName),
    ("FeedSource", FeedSource),
    ("FeedbackTemperatureRegulation", FeedbackTemperatureRegulation),
    ("Fellow", Fellow),
    ("Female", Female),
    ("FemaleChangedToMale", FemaleChangedToMale),
    ("FemalePseudohermaphrodite", FemalePseudohermaphrodite),
    ("FemaleTypical", FemaleTypical),
    ("FemoralHeadBallComponent", FemoralHeadBallComponent),
    ("FemoralHeadConeTaperComponent", FemoralHeadConeTaperComponent),
    ("FemoralHeadResurfacingComponent", FemoralHeadResurfacingComponent),
    ("FemoralStem", FemoralStem),
    ("FemoralStemComponent", FemoralStemComponent),
    ("FemoralStemDistalComponent", FemoralStemDistalComponent),
    ("FemoralStemProximalComponent", FemoralStemProximalComponent),
    ("FemurHeadCenterOfRotation", FemurHeadCenterOfRotation),
    ("FetalAnatomySurvey", FetalAnatomySurvey),
    ("FetalBiometry", FetalBiometry),
    ("FetalBiometryRatios", FetalBiometryRatios),
    ("FetalCardiacUltrasoundReport", FetalCardiacUltrasoundReport),
    ("FetalCardiovascularProfile", FetalCardiovascularProfile),
    ("FetalCardiovascularProfileScore", FetalCardiovascularProfileScore),
    ("FetalCranium", FetalCranium),
    ("FetalLongBones", FetalLongBones),
    ("FetalMeasurements", FetalMeasurements),
    ("Fetus", Fetus),
    ("FetusCharacteristics", FetusCharacteristics),
    ("FetusNumber", FetusNumber),
    ("FetusSummary", FetusSummary),
    ("FibroLipidicPlaque", FibroLipidicPlaque),
    ("Fibroadenolipoma", Fibroadenolipoma),
    ("FibroadenomatoidHyperplasia", FibroadenomatoidHyperplasia),
    ("Fibrocalcific", Fibrocalcific),
    ("Fibronodular", Fibronodular),
    ("FiducialAlignment", FiducialAlignment),
    ("FiducialFeature", FiducialFeature),
    ("FiducialIntent", FiducialIntent),
    ("FiducialMark", FiducialMark),
    ("Field1ForJoslin3Field", Field1ForJoslin3Field),
    ("Field2ForJoslin3Field", Field2ForJoslin3Field),
    ("Field3ForJoslin3Field", Field3ForJoslin3Field),
    ("FieldMapMRSignalIntensity", FieldMapMRSignalIntensity),
    ("FillerNumber", FillerNumber),
    ("FilmDigitizer", FilmDigitizer),
    ("FilmHolder", FilmHolder),
    ("FilmScreenMammography", FilmScreenMammography),
    ("Film_110010", Film_110010),
    ("Film_113951", Film_113951),
    ("FilteredBackProjection", FilteredBackProjection),
    ("Filtration", Filtration),
    ("FinalReport", FinalReport),
    ("Finding", Finding),
    ("FindingObservationType", FindingObservationType),
    ("Findings", Findings),
    ("FinePleomorphicCalcification", FinePleomorphicCalcification),
    ("FirmSkinLesion", FirmSkinLesion),
    ("FirstChordOfAbnormalRegion", FirstChordOfAbnormalRegion),
    ("FirstMammogramEver", FirstMammogramEver),
    ("FirstMeasureOfInformationCorrelationOfGLCM", FirstMeasureOfInformationCorrelationOfGLCM),
    ("FirstPassLeakageProfileModel", FirstPassLeakageProfileModel),
    ("FixationDeviceAngle", FixationDeviceAngle),
    ("FixationFalseNegativePercent", FixationFalseNegativePercent),
    ("FixationFalseNegativeRatio", FixationFalseNegativeRatio),
    ("FixationFalsePositivePercent", FixationFalsePositivePercent),
    ("FixationFalsePositiveRatio", FixationFalsePositiveRatio),
    ("FixationLossesRatio", FixationLossesRatio),
    ("FixationOrPositioningDevice", FixationOrPositioningDevice),
    ("FixedBeamDirection", FixedBeamDirection),
    ("FixedDp", FixedDp),
    ("FixedDurationPauseEnded", FixedDurationPauseEnded),
    ("FixedGrid", FixedGrid),
    ("FixedLaserSetupPoint", FixedLaserSetupPoint),
    ("FlashDoesntIncludeCassettePerScreenPerDetectorIdentification", FlashDoesntIncludeCassettePerScreenPerDetectorIdentification),
    ("FlashDoesntIncludeDateOfExamination", FlashDoesntIncludeDateOfExamination),
    ("FlashDoesntIncludeFacilityNameAndLocation", FlashDoesntIncludeFacilityNameAndLocation),
    ("FlashDoesntIncludeMammographyUnitIdentification", FlashDoesntIncludeMammographyUnitIdentification),
    ("FlashDoesntIncludePatientNameAndAdditionalPatientId", FlashDoesntIncludePatientNameAndAdditionalPatientId),
    ("FlashDoesntIncludeTechnologistIdentification", FlashDoesntIncludeTechnologistIdentification),
    ("FlashIsIllegibleDoesNotFitOrIsLopsided", FlashIsIllegibleDoesNotFitOrIsLopsided),
    ("FlashIsNotNearEdgeOfFilm", FlashIsNotNearEdgeOfFilm),
    ("FlatFilter", FlatFilter),
    ("FlatteningFilterBeam", FlatteningFilterBeam),
    ("FleischTable", FleischTable),
    ("FleischnersLineS", FleischnersLineS),
    ("Flocculent", Flocculent),
    ("FlorbetabenF18", FlorbetabenF18),
    ("FlowImageForImageProcessing", FlowImageForImageProcessing),
    ("FlowQuantification", FlowQuantification),
    ("FlowRateAboveAdjustmentLimit", FlowRateAboveAdjustmentLimit),
    ("FlowRateAboveWarningLimit", FlowRateAboveWarningLimit),
    ("FlowRateVsTime", FlowRateVsTime),
    ("FlowVariance", FlowVariance),
    ("FlowVelocity", FlowVelocity),
    ("FlubatineF18_126503", FlubatineF18_126503),
    ("FlubatineF18_126712", FlubatineF18_126712),
    ("Fluffy", Fluffy),
    ("FluidObtained", FluidObtained),
    ("FluoresceinAngiography", FluoresceinAngiography),
    ("FluoroDoseAreaProductTotal", FluoroDoseAreaProductTotal),
    ("FluoroDoseRPTotal", FluoroDoseRPTotal),
    ("FluoroMode", FluoroMode),
    ("FluoromisonidazoleFMISOF18", FluoromisonidazoleFMISOF18),
    ("FluoroscopyGuidedProjectionRadiographySystem", FluoroscopyGuidedProjectionRadiographySystem),
    ("FluorotrioprideF18", FluorotrioprideF18),
    ("FlurpiridazF18", FlurpiridazF18),
    ("FlushAdministration", FlushAdministration),
    ("FocalAbnormality", FocalAbnormality),
    ("FocalContrastEnhancement", FocalContrastEnhancement),
    ("FocalPlaneZOffset", FocalPlaneZOffset),
    ("FocalSpotSize", FocalSpotSize),
    ("FocusedGrid", FocusedGrid),
    ("FoldedTissue", FoldedTissue),
    ("FollicleCountGroup", FollicleCountGroup),
    ("FollicleCountGroupIdentifier", FollicleCountGroupIdentifier),
    ("FollicleType", FollicleType),
    ("FollowUpAtShortInterval111Months", FollowUpAtShortInterval111Months),
    ("FollowUpAtShortIntervalFromPriorStudy", FollowUpAtShortIntervalFromPriorStudy),
    ("FollowUpPostBiopsyAsDirectedByClinician", FollowUpPostBiopsyAsDirectedByClinician),
    ("FoodTreat", FoodTreat),
    ("FootToHead", FootToHead),
    ("ForAttenuationCorrection", ForAttenuationCorrection),
    ("ForClinicalTrialExport", ForClinicalTrialExport),
    ("ForClinicalTrialSubmission", ForClinicalTrialSubmission),
    ("ForConference", ForConference),
    ("ForDiagnosis", ForDiagnosis),
    ("ForEvaluation", ForEvaluation),
    ("ForLitigation", ForLitigation),
    ("ForMachineQualityAssurance", ForMachineQualityAssurance),
    ("ForPatient", ForPatient),
    ("ForPatientSetupVerification", ForPatientSetupVerification),
    ("ForPeerReview", ForPeerReview),
    ("ForPhysicianReview", ForPhysicianReview),
    ("ForPhysicistReview", ForPhysicistReview),
    ("ForPlanComparison", ForPlanComparison),
    ("ForPlanQualityAssurance", ForPlanQualityAssurance),
    ("ForPrinting", ForPrinting),
    ("ForProcessingPredecessor", ForProcessingPredecessor),
    ("ForPublicationExport", ForPublicationExport),
    ("ForRTPlanSummation", ForRTPlanSummation),
    ("ForRTPrescription", ForRTPrescription),
    ("ForRTTreatmentPlanning", ForRTTreatmentPlanning),
    ("ForRTWorkflow", ForRTWorkflow),
    ("ForReferringProvider", ForReferringProvider),
    ("ForReportAttachment", ForReportAttachment),
    ("ForResearch", ForResearch),
    ("ForResearchCollectionExport", ForResearchCollectionExport),
    ("ForSegmentation", ForSegmentation),
    ("ForSurgery", ForSurgery),
    ("ForTeaching", ForTeaching),
    ("ForTeachingFileExport", ForTeachingFileExport),
    ("ForTherapy", ForTherapy),
    ("ForTreatmentDelivery", ForTreatmentDelivery),
    ("ForTumorBoard", ForTumorBoard),
    ("ForTumorRegistry", ForTumorRegistry),
    ("ForVerification", ForVerification),
    ("ForcedAirHeater", ForcedAirHeater),
    ("ForeignBodyReaction", ForeignBodyReaction),
    ("ForeignMaterial", ForeignMaterial),
    ("FourChamberView", FourChamberView),
    ("FovealSensitivityRelated", FovealSensitivityRelated),
    ("Fp10", Fp10),
    ("Fp9", Fp9),
    ("FractalDimension", FractalDimension),
    ("FractionalAnisotropy", FractionalAnisotropy),
    ("FractionalChange", FractionalChange),
    ("FractionalKurtosisAnisotropy", FractionalKurtosisAnisotropy),
    ("FractionalOccupancySegmentation", FractionalOccupancySegmentation),
    ("FractionalProbabilisticSegmentation", FractionalProbabilisticSegmentation),
    ("FractionationBasedOrTemporallyBasedMethods", FractionationBasedOrTemporallyBasedMethods),
    ("FrameExtractingEquipment", FrameExtractingEquipment),
    ("FrameOfReferenceIdentity", FrameOfReferenceIdentity),
    ("FrameOfReferenceUID", FrameOfReferenceUID),
    ("FrameToFrameAnalysis", FrameToFrameAnalysis),
    ("FraminghamQTcAlgorithm", FraminghamQTcAlgorithm),
    ("FreeAcquisition", FreeAcquisition),
    ("FreeCordLoopMethod", FreeCordLoopMethod),
    ("FreeSilicone", FreeSilicone),
    ("Freehand", Freehand),
    ("FrequencyOfStimulusEvents", FrequencyOfStimulusEvents),
    ("Fresolimumab89Zr", Fresolimumab89Zr),
    ("FridericiaQTcAlgorithm", FridericiaQTcAlgorithm),
    ("FrontOfCorneaToFrontOfLens", FrontOfCorneaToFrontOfLens),
    ("FullBernoulli", FullBernoulli),
    ("FullCardiacCycle", FullCardiacCycle),
    ("FullFidelityImage", FullFidelityImage),
    ("FunctionalConditionPresentDuringAcquisition", FunctionalConditionPresentDuringAcquisition),
    ("FunctionalImage", FunctionalImage),
    ("Fundoscopy", Fundoscopy),
    ("GA20189Zr", GA20189Zr),
    ("GDPattern", GDPattern),
    ("GGraft", GGraft),
    ("GTVNodal", GTVNodal),
    ("GTVPrimary", GTVPrimary),
    ("Galactocele", Galactocele),
    ("GammaCapillaryTransitTimeModel", GammaCapillaryTransitTimeModel),
    ("GammaDistributionMode", GammaDistributionMode),
    ("GammaDistributionModel", GammaDistributionModel),
    ("GammaDistributionScaleParameter", GammaDistributionScaleParameter),
    ("GammaDistributionShapeParameter", GammaDistributionShapeParameter),
    ("GanglionCellAndInnerPlexiformLayers", GanglionCellAndInnerPlexiformLayers),
    ("GanglionCellComplex", GanglionCellComplex),
    ("GanglionCellComplexThickness", GanglionCellComplexThickness),
    ("GanglionCellLayerKeyMeasurements", GanglionCellLayerKeyMeasurements),
    ("GarwayHeathSectors", GarwayHeathSectors),
    ("GastrointestinalImagingSubjectMatter", GastrointestinalImagingSubjectMatter),
    ("GaussianBlur", GaussianBlur),
    ("GaussianFilter", GaussianFilter),
    ("GeneralMicroscopy", GeneralMicroscopy),
    ("GeneralReductionInSensitivity", GeneralReductionInSensitivity),
    ("Generic2DPlanning", Generic2DPlanning),
    ("Generic3DPlanning", Generic3DPlanning),
    ("GenericPlanningForHipReplacement", GenericPlanningForHipReplacement),
    ("GenericPlanningForKneeReplacement", GenericPlanningForKneeReplacement),
    ("GeneticModifications", GeneticModifications),
    ("GeneticModificationsDescription", GeneticModificationsDescription),
    ("GenitourinaryHistory", GenitourinaryHistory),
    ("GenitourinaryImagingSubjectMatter", GenitourinaryImagingSubjectMatter),
    ("GeographicAtrophyArea", GeographicAtrophyArea),
    ("GeometricCenterpoint", GeometricCenterpoint),
    ("GeometricIsocenter", GeometricIsocenter),
    ("GeometricNonIsocenter", GeometricNonIsocenter),
    ("GeometricPurposeOfRegion", GeometricPurposeOfRegion),
    ("GeometricRadiationTransportModel", GeometricRadiationTransportModel),
    ("GeometricalImageEvaluation", GeometricalImageEvaluation),
    ("GlaucomaHemifieldTestAnalysis", GlaucomaHemifieldTestAnalysis),
    ("GlembatumumabVedotin89Zr", GlembatumumabVedotin89Zr),
    ("Global", Global),
    ("GlomerularFiltrationRateBlackMDRD", GlomerularFiltrationRateBlackMDRD),
    ("GlomerularFiltrationRateCreatinineBasedFormulaSchwartz", GlomerularFiltrationRateCreatinineBasedFormulaSchwartz),
    ("GlomerularFiltrationRateCystatinBasedFormula", GlomerularFiltrationRateCystatinBasedFormula),
    ("GlomerularFiltrationRateFemaleMDRD", GlomerularFiltrationRateFemaleMDRD),
    ("GlomerularFiltrationRateMDRD", GlomerularFiltrationRateMDRD),
    ("GlomerularFiltrationRateNonBlackMDRD", GlomerularFiltrationRateNonBlackMDRD),
    ("GlucoseC11", GlucoseC11),
    ("GlucoseMeasurementDate", GlucoseMeasurementDate),
    ("GlucoseMeasurementTime", GlucoseMeasurementTime),
    ("GlutamateAndGlutamine", GlutamateAndGlutamine),
    ("GlutamineC11", GlutamineC11),
    ("GlutamineC14", GlutamineC14),
    ("GlutamineF18", GlutamineF18),
    ("Glycolysis", Glycolysis),
    ("GoodImageQuality", GoodImageQuality),
    ("GradientAssessment", GradientAssessment),
    ("GradientPressureAverage", GradientPressureAverage),
    ("GradientPressurePeak", GradientPressurePeak),
    ("GradientWeightedClassActivation", GradientWeightedClassActivation),
    ("GraftToCitedSegmentDistalSection", GraftToCitedSegmentDistalSection),
    ("GraftToCitedSegmentMidSection", GraftToCitedSegmentMidSection),
    ("GraftToCitedSegmentProximalSection", GraftToCitedSegmentProximalSection),
    ("GraftType", GraftType),
    ("GranularPattern", GranularPattern),
    ("GraphIncrement", GraphIncrement),
    ("Graticule", Graticule),
    ("GrayLevelCoOccurrenceMatrix", GrayLevelCoOccurrenceMatrix),
    ("GrayLevelDistanceZoneMatrix", GrayLevelDistanceZoneMatrix),
    ("GrayLevelNonuniformityInRuns", GrayLevelNonuniformityInRuns),
    ("GrayLevelNonuniformityInRunsNormalized", GrayLevelNonuniformityInRunsNormalized),
    ("GrayLevelNonuniformityOfZoneCounts", GrayLevelNonuniformityOfZoneCounts),
    ("GrayLevelNonuniformityOfZoneCountsNormalized", GrayLevelNonuniformityOfZoneCountsNormalized),
    ("GrayLevelRunLengthMatrix", GrayLevelRunLengthMatrix),
    ("GrayLevelSizeZoneMatrix", GrayLevelSizeZoneMatrix),
    ("GrayLevelVarianceInRuns", GrayLevelVarianceInRuns),
    ("GrayLevelVarianceInZones", GrayLevelVarianceInZones),
    ("GrayscaleResolutionEvaluation", GrayscaleResolutionEvaluation),
    ("GreaterThan1YearAgo", GreaterThan1YearAgo),
    ("GreenFilter", GreenFilter),
    ("GridArtifactS", GridArtifactS),
    ("GridBScanPattern", GridBScanPattern),
    ("GrossSpecimenImaging", GrossSpecimenImaging),
    ("GroundGlassOpacity", GroundGlassOpacity),
    ("GroupOfFramesForDisplay", GroupOfFramesForDisplay),
    ("GrowthPercentileRank", GrowthPercentileRank),
    ("GrowthZScore", GrowthZScore),
    ("GruesomeContent", GruesomeContent),
    ("GuidewireCrossingLesionSuccessful", GuidewireCrossingLesionSuccessful),
    ("GuidewireCrossingLesionUnsuccessful", GuidewireCrossingLesionUnsuccessful),
    ("GynecologicCondition", GynecologicCondition),
    ("GynecologicSurgery", GynecologicSurgery),
    ("HARDI", HARDI),
    ("HISBundleWave", HISBundleWave),
    ("HabitualVisualAcuity", HabitualVisualAcuity),
    ("Haigis", Haigis),
    ("HaigisA0", HaigisA0),
    ("HaigisA1", HaigisA1),
    ("HaigisA2", HaigisA2),
    ("HaigisL", HaigisL),
    ("HaigisLToric", HaigisLToric),
    ("HaigisToric", HaigisToric),
    ("HalfTimeForTissueRepair", HalfTimeForTissueRepair),
    ("HalfValueLayer", HalfValueLayer),
    ("HaloSign", HaloSign),
    ("HandGrips", HandGrips),
    ("HandGripsPresence", HandGripsPresence),
    ("HardCopy", HardCopy),
    ("HardCopyPrintServer", HardCopyPrintServer),
    ("HardWedge", HardWedge),
    ("HardwareConfiguration", HardwareConfiguration),
    ("HeadAndNeckImagingSpecialty", HeadAndNeckImagingSpecialty),
    ("HeadAndNeckMask", HeadAndNeckMask),
    ("HeadFixationBoard", HeadFixationBoard),
    ("HeadFootAxis", HeadFootAxis),
    ("HeadMask", HeadMask),
    ("HeadNodeSet", HeadNodeSet),
    ("HeadOfCardiology", HeadOfCardiology),
    ("HeadOfRadiology", HeadOfRadiology),
    ("HeadToFoot", HeadToFoot),
    ("Headframe", Headframe),
    ("HeadframePresent", HeadframePresent),
    ("HeartValveFlail", HeartValveFlail),
    ("HeatedImagingDevice", HeatedImagingDevice),
    ("HeatedPatientSupport", HeatedPatientSupport),
    ("HeatedWaterBlanket", HeatedWaterBlanket),
    ("HeatingConditions", HeatingConditions),
    ("HeelStop", HeelStop),
    ("HeelStopPosition", HeelStopPosition),
    ("Height", Height),
    ("HelicalBeam", HelicalBeam),
    ("HemangiomaNonparenchymalSubcutaneous", HemangiomaNonparenchymalSubcutaneous),
    ("HemisphericalUltrasoundTransducerGeometry", HemisphericalUltrasoundTransducerGeometry),
    ("Hemodynamic", Hemodynamic),
    ("HemodynamicResistance", HemodynamicResistance),
    ("HemodynamicResistanceIndex", HemodynamicResistanceIndex),
    ("HemodynamicsReport", HemodynamicsReport),
    ("HemostasisAchieved", HemostasisAchieved),
    ("HemostasisNotAchievedActivelyBleeding", HemostasisNotAchievedActivelyBleeding),
    ("HemostasisNotAchievedOozing", HemostasisNotAchievedOozing),
    ("Hermaphrodite", Hermaphrodite),
    ("HerniatedImplant", HerniatedImplant),
    ("HeterogeneousEchotexture", HeterogeneousEchotexture),
    ("Hiatus", Hiatus),
    ("High", High),
    ("HighDoseFractionLinearCellKillFactor", HighDoseFractionLinearCellKillFactor),
    ("HighDoseFractionTransitionDose", HighDoseFractionTransitionDose),
    ("HighGrayLevelRunEmphasis", HighGrayLevelRunEmphasis),
    ("HighGrayLevelZoneEmphasis", HighGrayLevelZoneEmphasis),
    ("HighResolution", HighResolution),
    ("HighShortAxisView", HighShortAxisView),
    ("HighThresholdEmptyTileSuppression", HighThresholdEmptyTileSuppression),
    ("HighThyroidUptake", HighThyroidUptake),
    ("HighlySuggestiveOfMalignancyTakeAppropriateAction", HighlySuggestiveOfMalignancyTakeAppropriateAction),
    ("HillRBF30", HillRBF30),
    ("HisBundleElectrogram", HisBundleElectrogram),
    ("HistogramAnalysis", HistogramAnalysis),
    ("HistologyUsingCoreBiopsy", HistologyUsingCoreBiopsy),
    ("HistoricalRTPrescription", HistoricalRTPrescription),
    ("History", History),
    ("HistoryOfBenignBreastBiopsy", HistoryOfBenignBreastBiopsy),
    ("HistoryOfBreastAugmentationAsymptomatic", HistoryOfBreastAugmentationAsymptomatic),
    ("HistoryOfEndometrialCancer", HistoryOfEndometrialCancer),
    ("HistoryOfHighRiskLesionOnPreviousBiopsy", HistoryOfHighRiskLesionOnPreviousBiopsy),
    ("HistoryOfMultipleFetuses", HistoryOfMultipleFetuses),
    ("HistoryOfNonMelanomaSkinCancer", HistoryOfNonMelanomaSkinCancer),
    ("HistoryOfOvarianCancer", HistoryOfOvarianCancer),
    ("HodgesQTcAlgorithm", HodgesQTcAlgorithm),
    ("HofferPACDConstant", HofferPACDConstant),
    ("HofferQ", HofferQ),
    ("Holladay1", Holladay1),
    ("Holladay2", Holladay2),
    ("HomogeneityFactor", HomogeneityFactor),
    ("Homogeneous", Homogeneous),
    ("HomogeneousFatEchotexture", HomogeneousFatEchotexture),
    ("HomogeneousFibroglandularEchotexture", HomogeneousFibroglandularEchotexture),
    ("HoneycombPattern", HoneycombPattern),
    ("HorizontalPixelSpacing", HorizontalPixelSpacing),
    ("HorizontalPrismBase", HorizontalPrismBase),
    ("HorizontalPrismPower", HorizontalPrismPower),
    ("HousingChangeInterval", HousingChangeInterval),
    ("HousingIndividuallyVentilated", HousingIndividuallyVentilated),
    ("HousingManufacturer", HousingManufacturer),
    ("HousingRackProductCode", HousingRackProductCode),
    ("HousingRackProductName", HousingRackProductName),
    ("HousingRole", HousingRole),
    ("HousingUnitHeight", HousingUnitHeight),
    ("HousingUnitLength", HousingUnitLength),
    ("HousingUnitLidProductCode", HousingUnitLidProductCode),
    ("HousingUnitLidProductName", HousingUnitLidProductName),
    ("HousingUnitLocationInRack", HousingUnitLocationInRack),
    ("HousingUnitMovement", HousingUnitMovement),
    ("HousingUnitProductCode", HousingUnitProductCode),
    ("HousingUnitProductName", HousingUnitProductName),
    ("HousingUnitReuse", HousingUnitReuse),
    ("HousingUnitWidth", HousingUnitWidth),
    ("HumanAlphaSynucleinPreformedFibrils", HumanAlphaSynucleinPreformedFibrils),
    ("HumanTauPreformedFibrils", HumanTauPreformedFibrils),
    ("HydrationVolume", HydrationVolume),
    ("HydropsFetalisScore", HydropsFetalisScore),
    ("HyperAcute", HyperAcute),
    ("HyperFractionation", HyperFractionation),
    ("Hyperechoic", Hyperechoic),
    ("HyperemicRatio", HyperemicRatio),
    ("HyperplasiaUsual", HyperplasiaUsual),
    ("HyperventilationBegin", HyperventilationBegin),
    ("HyperventilationEnd", HyperventilationEnd),
    ("HypoFractionation", HypoFractionation),
    ("Hypoechoic", Hypoechoic),
    ("HzAngular", HzAngular),
    ("HzNormalized", HzNormalized),
    ("IAUC", IAUC),
    ("IAUC180", IAUC180),
    ("IAUC180BN", IAUC180BN),
    ("IAUC60", IAUC60),
    ("IAUC60BN", IAUC60BN),
    ("IAUC90", IAUC90),
    ("IAUC90BN", IAUC90BN),
    ("IAUCBN", IAUCBN),
    ("ICRPPub103", ICRPPub103),
    ("ICRPPub60", ICRPPub60),
    ("ICRPPublication106", ICRPPublication106),
    ("ICRPPublication128", ICRPPublication128),
    ("ICRPPublication53", ICRPPublication53),
    ("ICRPPublication80", ICRPPublication80),
    ("IDPlate", IDPlate),
    ("IEC160mmHeadCTDosimetryPhantom", IEC160mmHeadCTDosimetryPhantom),
    ("IEC320mmBodyCTDosimetryPhantom", IEC320mmBodyCTDosimetryPhantom),
    ("IEC61217GantryContinuousPitchAngle", IEC61217GantryContinuousPitchAngle),
    ("IEC61217GantryContinuousRollAngle", IEC61217GantryContinuousRollAngle),
    ("IEC61217GantryContinuousYawAngle", IEC61217GantryContinuousYawAngle),
    ("IEC61217ImagingSourceToAxisDistance", IEC61217ImagingSourceToAxisDistance),
    ("IEC61217PatientSupportContinuousYawAngle", IEC61217PatientSupportContinuousYawAngle),
    ("IEC61217TableTopContinuousEccentricAngle", IEC61217TableTopContinuousEccentricAngle),
    ("IEC61217TableTopContinuousPitchAngle", IEC61217TableTopContinuousPitchAngle),
    ("IEC61217TableTopContinuousRollAngle", IEC61217TableTopContinuousRollAngle),
    ("IEC61217TableTopEccentricAxisDistance", IEC61217TableTopEccentricAxisDistance),
    ("IEC61217TableTopLateralPosition", IEC61217TableTopLateralPosition),
    ("IEC61217TableTopLongitudinalPosition", IEC61217TableTopLongitudinalPosition),
    ("IEC61217TableTopVerticalPosition", IEC61217TableTopVerticalPosition),
    ("IEC61217XRayImageReceptorLateralDisplacement", IEC61217XRayImageReceptorLateralDisplacement),
    ("IEC61217XRayImageReceptorLongitudinalDisplacement", IEC61217XRayImageReceptorLongitudinalDisplacement),
    ("IEC61217XRayImageReceptorRadialDisplacementFromIsocenter", IEC61217XRayImageReceptorRadialDisplacementFromIsocenter),
    ("IEC61217XRayImageReceptorRotation", IEC61217XRayImageReceptorRotation),
    ("IIRFilter", IIRFilter),
    ("ILCR", ILCR),
    ("IRIGBTimecode", IRIGBTimecode),
    ("ISUOG1stTrimester2023", ISUOG1stTrimester2023),
    ("ISUOG2ndTrimester2022", ISUOG2ndTrimester2022),
    ("ISUOG3rdTrimester2024", ISUOG3rdTrimester2024),
    ("ITV", ITV),
    ("IVCAWavePeakVelocity", IVCAWavePeakVelocity),
    ("IVCPreloadIndex", IVCPreloadIndex),
    ("IVCSPerA", IVCSPerA),
    ("IVCSWavePeakVelocity", IVCSWavePeakVelocity),
    ("IVUSReport", IVUSReport),
    ("IdentifiableEffectOnSurroundingTissues", IdentifiableEffectOnSurroundingTissues),
    ("IdentificationOfTheAttenuator", IdentificationOfTheAttenuator),
    ("IdentificationOfTheXRaySource", IdentificationOfTheXRaySource),
    ("Identifier", Identifier),
    ("IdentifierWithinPersonObserversRole", IdentifierWithinPersonObserversRole),
    ("IdentifyingContent", IdentifyingContent),
    ("IdentifyingSegment", IdentifyingSegment),
    ("IfAgeGreaterThan60yP50Equals293", IfAgeGreaterThan60yP50Equals293),
    ("IfAgeLessThan18yP50Equals272", IfAgeLessThan18yP50Equals272),
    ("IfAgeLessThan1dayP50Equals216", IfAgeLessThan1dayP50Equals216),
    ("IfAgeLessThan30dayP50Equals246", IfAgeLessThan30dayP50Equals246),
    ("IfAgeLessThan40yP50Equals274", IfAgeLessThan40yP50Equals274),
    ("IfPremAgeLessThan3daysP50Equals199", IfPremAgeLessThan3daysP50Equals199),
    ("Igloo", Igloo),
    ("IlluminationMethod", IlluminationMethod),
    ("IlluminationSpectralBand", IlluminationSpectralBand),
    ("IlluminationWavelength", IlluminationWavelength),
    ("IllustrationOfFinding", IllustrationOfFinding),
    ("IllustrationOfROI", IllustrationOfROI),
    ("ImageAcquired", ImageAcquired),
    ("ImageAcquiredDuringTreatment", ImageAcquiredDuringTreatment),
    ("ImageArtifactS", ImageArtifactS),
    ("ImageCapture", ImageCapture),
    ("ImageContentAndFiducialBasedAlignment", ImageContentAndFiducialBasedAlignment),
    ("ImageContentBasedAlignment", ImageContentBasedAlignment),
    ("ImageDetectedMass", ImageDetectedMass),
    ("ImageDetector", ImageDetector),
    ("ImageFrame", ImageFrame),
    ("ImageInterval", ImageInterval),
    ("ImageLaterality", ImageLaterality),
    ("ImageLibrary", ImageLibrary),
    ("ImageLibraryGroup", ImageLibraryGroup),
    ("ImageOrientationPatientColumnX", ImageOrientationPatientColumnX),
    ("ImageOrientationPatientColumnY", ImageOrientationPatientColumnY),
    ("ImageOrientationPatientColumnZ", ImageOrientationPatientColumnZ),
    ("ImageOrientationPatientRowX", ImageOrientationPatientRowX),
    ("ImageOrientationPatientRowY", ImageOrientationPatientRowY),
    ("ImageOrientationPatientRowZ", ImageOrientationPatientRowZ),
    ("ImagePositionPatientX", ImagePositionPatientX),
    ("ImagePositionPatientY", ImagePositionPatientY),
    ("ImagePositionPatientZ", ImagePositionPatientZ),
    ("ImageProcessing", ImageProcessing),
    ("ImageQuality", ImageQuality),
    ("ImageQualityController", ImageQualityController),
    ("ImageQualityRating", ImageQualityRating),
    ("ImageQualityReferenceParameters", ImageQualityReferenceParameters),
    ("ImageReconstruction", ImageReconstruction),
    ("ImageRegion", ImageRegion),
    ("ImageSetProperties", ImageSetProperties),
    ("ImageSetQualityRating", ImageSetQualityRating),
    ("ImageSpaceMAR", ImageSpaceMAR),
    ("ImageSpecificity", ImageSpecificity),
    ("ImageStitching", ImageStitching),
    ("ImageType", ImageType),
    ("ImageUsedAsReferenceImageForTreatment", ImageUsedAsReferenceImageForTreatment),
    ("ImageUsedForDoseCalculation", ImageUsedForDoseCalculation),
    ("ImageUsedForTreatmentPlanning", ImageUsedForTreatmentPlanning),
    ("ImageView", ImageView),
    ("ImageViewModifier", ImageViewModifier),
    ("ImagedContainerLabel", ImagedContainerLabel),
    ("ImagesRelatedToStandaloneObject", ImagesRelatedToStandaloneObject),
    ("Imagify", Imagify),
    ("ImagingAgentAdministrationActivity", ImagingAgentAdministrationActivity),
    ("ImagingAgentAdministrationAdverseEvents", ImagingAgentAdministrationAdverseEvents),
    ("ImagingAgentAdministrationCompletionStatus", ImagingAgentAdministrationCompletionStatus),
    ("ImagingAgentAdministrationConsumable", ImagingAgentAdministrationConsumable),
    ("ImagingAgentAdministrationConsumableType", ImagingAgentAdministrationConsumableType),
    ("ImagingAgentAdministrationDelay", ImagingAgentAdministrationDelay),
    ("ImagingAgentAdministrationGraph", ImagingAgentAdministrationGraph),
    ("ImagingAgentAdministrationInjectorEventType", ImagingAgentAdministrationInjectorEventType),
    ("ImagingAgentAdministrationInjectorEvents", ImagingAgentAdministrationInjectorEvents),
    ("ImagingAgentAdministrationInjectorPhaseIdentifier", ImagingAgentAdministrationInjectorPhaseIdentifier),
    ("ImagingAgentAdministrationPerformedPhaseUID", ImagingAgentAdministrationPerformedPhaseUID),
    ("ImagingAgentAdministrationPerformedStepUID", ImagingAgentAdministrationPerformedStepUID),
    ("ImagingAgentAdministrationPhase", ImagingAgentAdministrationPhase),
    ("ImagingAgentAdministrationPhaseIdentifier", ImagingAgentAdministrationPhaseIdentifier),
    ("ImagingAgentAdministrationPhaseType", ImagingAgentAdministrationPhaseType),
    ("ImagingAgentAdministrationPhaseWithManualHold", ImagingAgentAdministrationPhaseWithManualHold),
    ("ImagingAgentAdministrationStep", ImagingAgentAdministrationStep),
    ("ImagingAgentAdministrationStepIdentifier", ImagingAgentAdministrationStepIdentifier),
    ("ImagingAgentAdministrationStepSequenceNumber", ImagingAgentAdministrationStepSequenceNumber),
    ("ImagingAgentAdministrationSteps", ImagingAgentAdministrationSteps),
    ("ImagingAgentAdministrationStepsDescription", ImagingAgentAdministrationStepsDescription),
    ("ImagingAgentAdministrationStepsName", ImagingAgentAdministrationStepsName),
    ("ImagingAgentComponent", ImagingAgentComponent),
    ("ImagingAgentComponentUsage", ImagingAgentComponentUsage),
    ("ImagingAgentIdentifier", ImagingAgentIdentifier),
    ("ImagingAgentInformation", ImagingAgentInformation),
    ("ImagingAgentVolumePerUnitOfPresentation", ImagingAgentVolumePerUnitOfPresentation),
    ("ImagingAgentWarmed", ImagingAgentWarmed),
    ("ImagingConditions", ImagingConditions),
    ("ImagingDirection", ImagingDirection),
    ("ImagingMeasurementReport", ImagingMeasurementReport),
    ("ImagingMeasurements", ImagingMeasurements),
    ("ImagingProcedureRoom", ImagingProcedureRoom),
    ("ImagingSeriesQuality", ImagingSeriesQuality),
    ("ImagingStartDateTime", ImagingStartDateTime),
    ("ImagingStudyQuality", ImagingStudyQuality),
    ("ImpairedRenalFunction", ImpairedRenalFunction),
    ("ImplantAssemblyTemplate", ImplantAssemblyTemplate),
    ("ImplantComponentList", ImplantComponentList),
    ("ImplantFabrication", ImplantFabrication),
    ("ImplantFinding", ImplantFinding),
    ("ImplantTemplate", ImplantTemplate),
    ("ImplantType", ImplantType),
    ("ImplantationPatientPositioning", ImplantationPatientPositioning),
    ("ImplantationPlan", ImplantationPlan),
    ("Import", Import),
    ("Impression", Impression),
    ("ImpressionDescription", ImpressionDescription),
    ("Impressions", Impressions),
    ("InDetectorPlane", InDetectorPlane),
    ("InFocus", InFocus),
    ("InHomeCage", InHomeCage),
    ("InStentNeointima", InStentNeointima),
    ("InStentNeointimalCrossSectionalArea", InStentNeointimalCrossSectionalArea),
    ("InStentNeointimalVolume", InStentNeointimalVolume),
    ("InadequateCompression", InadequateCompression),
    ("InadequateContrastEnhancement", InadequateContrastEnhancement),
    ("InadequatelyStained", InadequatelyStained),
    ("InappropriateForTheDevice", InappropriateForTheDevice),
    ("InappropriateForTheIndications", InappropriateForTheIndications),
    ("InappropriateImageProcessing", InappropriateImageProcessing),
    ("InappropriatePatientOrientation", InappropriatePatientOrientation),
    ("InappropriatePatientPosition", InappropriatePatientPosition),
    ("IncisionMade", IncisionMade),
    ("IncompleteStudy", IncompleteStudy),
    ("InconsistentWithLabelingOfTheDevice", InconsistentWithLabelingOfTheDevice),
    ("IncorrectModalityWorklistEntry", IncorrectModalityWorklistEntry),
    ("IncorrectProcedureOrdered", IncorrectProcedureOrdered),
    ("IncorrectSideOrdered", IncorrectSideOrdered),
    ("IncorrectWorklistEntrySelected", IncorrectWorklistEntrySelected),
    ("IncrementalAcquisitionTriggering", IncrementalAcquisitionTriggering),
    ("IndependentRadiationShield", IndependentRadiationShield),
    ("Index", Index),
    ("Indexed", Indexed),
    ("IndexedHemodynamicResistance", IndexedHemodynamicResistance),
    ("IndicatedProblem", IndicatedProblem),
    ("IndicationsForPharmacologicalStress", IndicationsForPharmacologicalStress),
    ("IndicationsForProcedure", IndicationsForProcedure),
    ("IndicatorAppearanceTime", IndicatorAppearanceTime),
    ("IndicatorMeanTransitTime", IndicatorMeanTransitTime),
    ("IndirectDetector", IndirectDetector),
    ("IndividualCalcification", IndividualCalcification),
    ("IndividualImpressionPerRecommendation", IndividualImpressionPerRecommendation),
    ("IndividualImpressionPerRecommendationAnalysis", IndividualImpressionPerRecommendationAnalysis),
    ("IneligibleForReimbursement", IneligibleForReimbursement),
    ("InferiorToAnterior", InferiorToAnterior),
    ("InferolateralToSuperomedial", InferolateralToSuperomedial),
    ("InferomedialToSuperolateral", InferomedialToSuperolateral),
    ("Infiltrate", Infiltrate),
    ("InflatableEndorectalCoilFillSubstance", InflatableEndorectalCoilFillSubstance),
    ("InflowAngle", InflowAngle),
    ("InformationTechnology", InformationTechnology),
    ("InformationUsedForPlanning", InformationUsedForPlanning),
    ("InfraredFilter", InfraredFilter),
    ("InfraredMarker", InfraredMarker),
    ("InfraredReflectorMarker", InfraredReflectorMarker),
    ("InfusateEnd", InfusateEnd),
    ("InfusateStart", InfusateStart),
    ("InfusionSystem", InfusionSystem),
    ("Inhomogeneous", Inhomogeneous),
    ("InitialVolumeOfImagingAgentInContainer", InitialVolumeOfImagingAgentInContainer),
    ("InitiatorRoleID", InitiatorRoleID),
    ("InjectateTemperature", InjectateTemperature),
    ("InjectateVolume", InjectateVolume),
    ("InjectorEventDetectionDateTime", InjectorEventDetectionDateTime),
    ("InletIncluded", InletIncluded),
    ("InnerLimitsOfFuzzyMargin", InnerLimitsOfFuzzyMargin),
    ("InnerSurfaceOfTheEllipsoidZone", InnerSurfaceOfTheEllipsoidZone),
    ("InnerSurfaceOfTheInterdigitationZone", InnerSurfaceOfTheInterdigitationZone),
    ("InnerSurfaceOfTheRPE", InnerSurfaceOfTheRPE),
    ("InsideOperationalLimitsOfTheDevice", InsideOperationalLimitsOfTheDevice),
    ("InstanceNumber", InstanceNumber),
    ("InstancesImported", InstancesImported),
    ("InstitutionallyApprovedEstimates", InstitutionallyApprovedEstimates),
    ("InstitutionallyDefinedQualityControlStandard", InstitutionallyDefinedQualityControlStandard),
    ("InstrumentApproach", InstrumentApproach),
    ("InsufficientImplantDisplacementIncorrect", InsufficientImplantDisplacementIncorrect),
    ("InsufficientQualityForInterpretation", InsufficientQualityForInterpretation),
    ("IntegratedProjectionRadiographySystem", IntegratedProjectionRadiographySystem),
    ("IntegrationOfSumOfClosedAreasOnContiguousSlicesMethodForVolume", IntegrationOfSumOfClosedAreasOnContiguousSlicesMethodForVolume),
    ("InterHemisphericPlane", InterHemisphericPlane),
    ("InterMarkerDistance", InterMarkerDistance),
    ("Interface", Interface),
    ("InterferingTearsOrDrops", InterferingTearsOrDrops),
    ("InterferometricTransducer", InterferometricTransducer),
    ("Interferometry", Interferometry),
    ("InterferometryBasedCornealTomographer", InterferometryBasedCornealTomographer),
    ("InterlockOverridden", InterlockOverridden),
    ("InterlockResolved", InterlockResolved),
    ("IntermediateFamilyHistoryOfBreastCancer", IntermediateFamilyHistoryOfBreastCancer),
    ("IntermediateLevel", IntermediateLevel),
    ("Intern", Intern),
    ("InternalOsDistance", InternalOsDistance),
    ("InternationalProgressiveDogBreedersAlliance", InternationalProgressiveDogBreedersAlliance),
    ("InterpolatedLocalReference", InterpolatedLocalReference),
    ("Interpretation", Interpretation),
    ("InterpretationRequiresSpecialistExpertise", InterpretationRequiresSpecialistExpertise),
    ("InterquartileRangeOfPopulation", InterquartileRangeOfPopulation),
    ("InterquartileRangeOfUltrasoundAttenuationCoefficient", InterquartileRangeOfUltrasoundAttenuationCoefficient),
    ("InterquartileRangeToMedianRatioOfPopulation", InterquartileRangeToMedianRatioOfPopulation),
    ("InterquartileRangeToMedianRatioOfUltrasoundAttenCoeff", InterquartileRangeToMedianRatioOfUltrasoundAttenCoeff),
    ("InterstitialIllumination", InterstitialIllumination),
    ("InterventionAction", InterventionAction),
    ("InterventionAttemptIdentifier", InterventionAttemptIdentifier),
    ("InterventionPerformed", InterventionPerformed),
    ("InterventionalDevicePlaced", InterventionalDevicePlaced),
    ("InterventionalDevicePlacementUnsuccessful", InterventionalDevicePlacementUnsuccessful),
    ("InterventionalDeviceWithdrawn", InterventionalDeviceWithdrawn),
    ("IntimalDissection", IntimalDissection),
    ("IntraOralRadiography", IntraOralRadiography),
    ("IntraProceduralStructuralHeartMeasurementReport", IntraProceduralStructuralHeartMeasurementReport),
    ("IntraStentDissection", IntraStentDissection),
    ("IntracysticLesion", IntracysticLesion),
    ("IntracysticPapillaryCarcinoma", IntracysticPapillaryCarcinoma),
    ("IntracysticPapilloma", IntracysticPapilloma),
    ("IntraductalCarcinomaHighGrade", IntraductalCarcinomaHighGrade),
    ("IntraductalCarcinomaLowGrade", IntraductalCarcinomaLowGrade),
    ("IntraductalCarcinomaMicroPapillary", IntraductalCarcinomaMicroPapillary),
    ("IntraductalComedocarcinomaWithNecrosis", IntraductalComedocarcinomaWithNecrosis),
    ("IntralobularLines", IntralobularLines),
    ("IntraluminalFillingDefect", IntraluminalFillingDefect),
    ("IntraocularLensCalculation", IntraocularLensCalculation),
    ("IntraoperativeFixedAperture", IntraoperativeFixedAperture),
    ("IntraoperativeTransducer", IntraoperativeTransducer),
    ("IntravascularOpticalCoherenceTomography", IntravascularOpticalCoherenceTomography),
    ("IntravascularTransducer", IntravascularTransducer),
    ("IntravascularUltrasound", IntravascularUltrasound),
    ("IntravenousExtravasationSymptoms", IntravenousExtravasationSymptoms),
    ("InvasiveAndInSituCarcinoma", InvasiveAndInSituCarcinoma),
    ("InvasiveBreastCarcinoma", InvasiveBreastCarcinoma),
    ("InvasiveCribriformCarcinoma", InvasiveCribriformCarcinoma),
    ("InvasiveLobularCarcinoma", InvasiveLobularCarcinoma),
    ("InverseDifferenceMomentNormalizedOfGLCM", InverseDifferenceMomentNormalizedOfGLCM),
    ("InverseDifferenceMomentOfGLCM", InverseDifferenceMomentOfGLCM),
    ("InverseDifferenceNormalizedOfGLCM", InverseDifferenceNormalizedOfGLCM),
    ("InverseDifferenceOfGLCM", InverseDifferenceOfGLCM),
    ("InverseVarianceOfGLCM", InverseVarianceOfGLCM),
    ("InvestigationalNewDrug", InvestigationalNewDrug),
    ("Involved", Involved),
    ("IodinatedI125DPA713", IodinatedI125DPA713),
    ("IonChamber", IonChamber),
    ("IonChamberArray", IonChamberArray),
    ("IonTherapyParticle", IonTherapyParticle),
    ("IrradiatingDevice", IrradiatingDevice),
    ("IrradiationAdministering", IrradiationAdministering),
    ("IrradiationAuthorizing", IrradiationAuthorizing),
    ("IrradiationDetails", IrradiationDetails),
    ("IrradiationDuration", IrradiationDuration),
    ("IrradiationEvent", IrradiationEvent),
    ("IrradiationEventLabel", IrradiationEventLabel),
    ("IrradiationEventSummaryData", IrradiationEventSummaryData),
    ("IrradiationEventType", IrradiationEventType),
    ("IrradiationEventUID", IrradiationEventUID),
    ("IrradiationEventUID_113853", IrradiationEventUID_113853),
    ("IrradiationEventXRayData", IrradiationEventXRayData),
    ("IsIonic", IsIonic),
    ("IsRejectedAcquisition", IsRejectedAcquisition),
    ("IsRepeatedAcquisition", IsRepeatedAcquisition),
    ("IsocenterOrigin", IsocenterOrigin),
    ("IsocentricPatientSupportContinuousPitchAngle", IsocentricPatientSupportContinuousPitchAngle),
    ("IsocentricPatientSupportContinuousRollAngle", IsocentricPatientSupportContinuousRollAngle),
    ("IsocentricPatientSupportContinuousYawAngle", IsocentricPatientSupportContinuousYawAngle),
    ("IsocentricPatientSupportLateralPosition", IsocentricPatientSupportLateralPosition),
    ("IsocentricPatientSupportLongitudinalPosition", IsocentricPatientSupportLongitudinalPosition),
    ("IsocentricPatientSupportVerticalPosition", IsocentricPatientSupportVerticalPosition),
    ("IsocentricSetupMethod", IsocentricSetupMethod),
    ("IsocentricTreatmentLocationPoint", IsocentricTreatmentLocationPoint),
    ("Isodose", Isodose),
    ("IsodoseVolume", IsodoseVolume),
    ("Isoechoic", Isoechoic),
    ("IssuerOfIdentifier", IssuerOfIdentifier),
    ("IssuerOfParentSpecimenIdentifier", IssuerOfParentSpecimenIdentifier),
    ("IssuerOfSpecimenIdentifier", IssuerOfSpecimenIdentifier),
    ("IterativeClosestPoint", IterativeClosestPoint),
    ("IterativeMAR", IterativeMAR),
    ("IterativeReconstruction", IterativeReconstruction),
    ("J59189Zr", J59189Zr),
    ("JDMSFetalAnatomy2014", JDMSFetalAnatomy2014),
    ("JSUMFetalMorphology2021", JSUMFetalMorphology2021),
    ("JWire", JWire),
    ("JaccardIndex", JaccardIndex),
    ("JawPair", JawPair),
    ("JawPositionToleranceViolation", JawPositionToleranceViolation),
    ("JointAverageOfGLCM", JointAverageOfGLCM),
    ("JointEntropyOfGLCM", JointEntropyOfGLCM),
    ("JointMaximumOfGLCM", JointMaximumOfGLCM),
    ("JointPositionMethod", JointPositionMethod),
    ("JointVarianceOfGLCM", JointVarianceOfGLCM),
    ("JugularLymphaticSac", JugularLymphaticSac),
    ("JuvenilePapillomatosis", JuvenilePapillomatosis),
    ("KTypeFilter", KTypeFilter),
    ("KVP", KVP),
    ("Kane", Kane),
    ("KaneKeratoconus", KaneKeratoconus),
    ("KaneToric", KaneToric),
    ("KeepVeinOpenEnded", KeepVeinOpenEnded),
    ("KeepVeinOpenStarted", KeepVeinOpenStarted),
    ("Kep", Kep),
    ("Keratometry", Keratometry),
    ("KeratometryMeasurementsSOPInstance", KeratometryMeasurementsSOPInstance),
    ("KerleyALine", KerleyALine),
    ("KerleyBLine", KerleyBLine),
    ("KerleyCLines", KerleyCLines),
    ("KeyImages", KeyImages),
    ("KeyObjectDescription", KeyObjectDescription),
    ("KeyObjectSelection", KeyObjectSelection),
    ("KidneyStent", KidneyStent),
    ("KnownBiopsyProvenMalignancy", KnownBiopsyProvenMalignancy),
    ("KnownBiopsyProvenMalignancyTakeAppropriateAction", KnownBiopsyProvenMalignancyTakeAppropriateAction),
    ("Krishnan2016", Krishnan2016),
    ("Ktrans", Ktrans),
    ("Kurtosis", Kurtosis),
    ("KurtosisDiffusionCoefficient", KurtosisDiffusionCoefficient),
    ("KurtosisDiffusionModel", KurtosisDiffusionModel),
    ("LADRegionInLAOProjection", LADRegionInLAOProjection),
    ("LADRegionInRAOProjection", LADRegionInRAOProjection),
    ("LBWOrIUGR", LBWOrIUGR),
    ("LDLCholesterolScoreSheetForMen", LDLCholesterolScoreSheetForMen),
    ("LDLCholesterolScoreSheetForWomen", LDLCholesterolScoreSheetForWomen),
    ("LSDI", LSDI),
    ("LVEndSystolicGLS2CHEndocardial", LVEndSystolicGLS2CHEndocardial),
    ("LVEndSystolicGLS4CHEndocardial", LVEndSystolicGLS4CHEndocardial),
    ("LVEndSystolicGLSALAXEndocardial", LVEndSystolicGLSALAXEndocardial),
    ("LVEndSystolicGLSMultiviewMeanEndocardial", LVEndSystolicGLSMultiviewMeanEndocardial),
    ("LVEndSystolicMechanicalDispersion", LVEndSystolicMechanicalDispersion),
    ("LVPeakGLS2CHEndocardial", LVPeakGLS2CHEndocardial),
    ("LVPeakGLS4CHEndocardial", LVPeakGLS4CHEndocardial),
    ("LVPeakGLSALAXEndocardial", LVPeakGLSALAXEndocardial),
    ("LVPeakGLSMultiviewMeanEndocardial", LVPeakGLSMultiviewMeanEndocardial),
    ("LVPeakSystolicGLS2CHEndocardial", LVPeakSystolicGLS2CHEndocardial),
    ("LVPeakSystolicGLS4CHEndocardial", LVPeakSystolicGLS4CHEndocardial),
    ("LVPeakSystolicGLSALAXEndocardial", LVPeakSystolicGLSALAXEndocardial),
    ("LVPeakSystolicGLSMultiviewMeanEndocardial", LVPeakSystolicGLSMultiviewMeanEndocardial),
    ("LVWallMotionScoreIndex", LVWallMotionScoreIndex),
    ("LWHMethodForVolumeOfEllipsoid", LWHMethodForVolumeOfEllipsoid),
    ("LabelType", LabelType),
    ("LactatingAdenoma", LactatingAdenoma),
    ("LactationalChange", LactationalChange),
    ("LandmarkBasedWallMotionAnalysis", LandmarkBasedWallMotionAnalysis),
    ("Language", Language),
    ("LanguageOfContentItemAndDescendants", LanguageOfContentItemAndDescendants),
    ("LanguageOfNameAndValue", LanguageOfNameAndValue),
    ("LanguageOfValue", LanguageOfValue),
    ("Laparoscopy", Laparoscopy),
    ("LargeDuctPapilloma", LargeDuctPapilloma),
    ("LargeGreaterThan380CmLateralThickness", LargeGreaterThan380CmLateralThickness),
    ("LargeZoneEmphasis", LargeZoneEmphasis),
    ("LargeZoneHighGrayLevelEmphasis", LargeZoneHighGrayLevelEmphasis),
    ("LargeZoneLowGrayLevelEmphasis", LargeZoneLowGrayLevelEmphasis),
    ("LaserCrossHairs", LaserCrossHairs),
    ("LaserScanning", LaserScanning),
    ("LaserScanning3DCAMModel", LaserScanning3DCAMModel),
    ("LaserSurfaceScan", LaserSurfaceScan),
    ("LastChordOfAbnormalRegion", LastChordOfAbnormalRegion),
    ("LateChildBearingAfter30", LateChildBearingAfter30),
    ("LateContrastEnhancement", LateContrastEnhancement),
    ("LeadID", LeadID),
    ("LeadRadiologicTechnologist", LeadRadiologicTechnologist),
    ("LeafPairs", LeafPairs),
    ("LeastMeanSquareDeconvolution", LeastMeanSquareDeconvolution),
    ("LeastSquaresFitOfMultipleSamples", LeastSquaresFitOfMultipleSamples),
    ("LeftArmRestPitchAngle", LeftArmRestPitchAngle),
    ("LeftArmRestPosition", LeftArmRestPosition),
    ("LeftArmRestRollAngle", LeftArmRestRollAngle),
    ("LeftAtrialAppendageClosureDevice", LeftAtrialAppendageClosureDevice),
    ("LeftAtrialAppendageClosureDeviceCircumference", LeftAtrialAppendageClosureDeviceCircumference),
    ("LeftAtrialAppendageClosureDeviceCompressionRatio", LeftAtrialAppendageClosureDeviceCompressionRatio),
    ("LeftAtrialAppendageClosureDeviceDiameter", LeftAtrialAppendageClosureDeviceDiameter),
    ("LeftAtrialAppendageClosureDeviceSize", LeftAtrialAppendageClosureDeviceSize),
    ("LeftAtrialAppendageDepth", LeftAtrialAppendageDepth),
    ("LeftAtrialAppendageLandingZoneMajorAxis", LeftAtrialAppendageLandingZoneMajorAxis),
    ("LeftAtrialAppendageMajorAxis", LeftAtrialAppendageMajorAxis),
    ("LeftAtrialAppendageMinorAxis", LeftAtrialAppendageMinorAxis),
    ("LeftAtrialAppendageOstiumPerimeter", LeftAtrialAppendageOstiumPerimeter),
    ("LeftAtrialEDVolume", LeftAtrialEDVolume),
    ("LeftAtrialESVolume", LeftAtrialESVolume),
    ("LeftAtrialEjectionFractionByAngiography", LeftAtrialEjectionFractionByAngiography),
    ("LeftAtriumDescendingAortaDistance", LeftAtriumDescendingAortaDistance),
    ("LeftContour", LeftContour),
    ("LeftEyeRx", LeftEyeRx),
    ("LeftFirst", LeftFirst),
    ("LeftHemisphereMostAnterior", LeftHemisphereMostAnterior),
    ("LeftHemisphereMostInferior", LeftHemisphereMostInferior),
    ("LeftHemisphereMostPosterior", LeftHemisphereMostPosterior),
    ("LeftHemisphereMostSuperior", LeftHemisphereMostSuperior),
    ("LeftMainCoronaryOstiumHeight", LeftMainCoronaryOstiumHeight),
    ("LeftRightAxis", LeftRightAxis),
    ("LeftToRight", LeftToRight),
    ("LeftToRightFlow", LeftToRightFlow),
    ("LeftVentricleApicalAnterolateralSegment", LeftVentricleApicalAnterolateralSegment),
    ("LeftVentricleApicalAnteroseptalSegment", LeftVentricleApicalAnteroseptalSegment),
    ("LeftVentricleApicalInferolateralSegment", LeftVentricleApicalInferolateralSegment),
    ("LeftVentricleApicalInferoseptalSegment", LeftVentricleApicalInferoseptalSegment),
    ("LeftVentricleMassByAreaLength", LeftVentricleMassByAreaLength),
    ("LeftVentricleMassByAreaLengthAdjustedByHeight", LeftVentricleMassByAreaLengthAdjustedByHeight),
    ("LeftVentricleMassByMMode", LeftVentricleMassByMMode),
    ("LeftVentricleMassByMModeAdjustedByHeight", LeftVentricleMassByMModeAdjustedByHeight),
    ("LeftVentricleMassByTruncatedEllipse", LeftVentricleMassByTruncatedEllipse),
    ("LeftVentricleMassByTruncatedEllipseAdjustedByHeight", LeftVentricleMassByTruncatedEllipseAdjustedByHeight),
    ("LeftVentricularEjectionTime", LeftVentricularEjectionTime),
    ("LeftVentricularFillingTime", LeftVentricularFillingTime),
    ("LeftVentricularInletLength", LeftVentricularInletLength),
    ("LeftVentricularOutflowTractCalcificationSeverity", LeftVentricularOutflowTractCalcificationSeverity),
    ("LeftVentricularOutflowTractView", LeftVentricularOutflowTractView),
    ("LeftVentricularPosterobasalSegment", LeftVentricularPosterobasalSegment),
    ("LengthLuminalSegment", LengthLuminalSegment),
    ("LengthOfReconstructableVolume", LengthOfReconstructableVolume),
    ("Lensometry", Lensometry),
    ("LesionBoundary", LesionBoundary),
    ("LesionCapsularContactLength", LesionCapsularContactLength),
    ("LesionCentered", LesionCentered),
    ("LesionCompletelyRemoved", LesionCompletelyRemoved),
    ("LesionDensity", LesionDensity),
    ("LesionIdentifier", LesionIdentifier),
    ("LesionInterventionInformation", LesionInterventionInformation),
    ("LesionMorphology", LesionMorphology),
    ("LesionPartiallyRemoved", LesionPartiallyRemoved),
    ("LesionRisk", LesionRisk),
    ("LesionSensitivity", LesionSensitivity),
    ("LesionToBackgroundSUVRatio", LesionToBackgroundSUVRatio),
    ("LessThan3MonthsAgo", LessThan3MonthsAgo),
    ("LeukemicInfiltration", LeukemicInfiltration),
    ("LevelOfDifficulty", LevelOfDifficulty),
    ("LevelOfSignificance", LevelOfSignificance),
    ("LevenbergMarquardt", LevenbergMarquardt),
    ("Levovist", Levovist),
    ("LightBrownColor", LightBrownColor),
    ("LightsOnTimeOfDay", LightsOnTimeOfDay),
    ("Line", Line),
    ("LineBScanPattern", LineBScanPattern),
    ("LineNoiseArtifact", LineNoiseArtifact),
    ("LineScanPattern", LineScanPattern),
    ("LineSegmentLength", LineSegmentLength),
    ("Linear", Linear),
    ("LinearBoltzmannTransportEquation", LinearBoltzmannTransportEquation),
    ("LinearCellKillFactor", LinearCellKillFactor),
    ("LinearCurve", LinearCurve),
    ("LinearDisplacement", LinearDisplacement),
    ("LinearQuadraticLinearModel", LinearQuadraticLinearModel),
    ("LinearQuadraticModel", LinearQuadraticModel),
    ("LinearQuadraticModelForLowDoseRateBrachytherapy", LinearQuadraticModelForLowDoseRateBrachytherapy),
    ("LinearQuadraticModelWithTimeFactor", LinearQuadraticModelWithTimeFactor),
    ("LinearSpiculation", LinearSpiculation),
    ("LinearUltrasoundTransducerGeometry", LinearUltrasoundTransducerGeometry),
    ("LipidAndLactate", LipidAndLactate),
    ("LiquidCrystalDisplay", LiquidCrystalDisplay),
    ("LiquidIonChamber", LiquidIonChamber),
    ("Lobar", Lobar),
    ("Lobulated", Lobulated),
    ("LocalEffectModel", LocalEffectModel),
    ("LocalServiceOperationStarted", LocalServiceOperationStarted),
    ("LocalServiceOperationStopped", LocalServiceOperationStopped),
    ("LocalizedProstateFinding", LocalizedProstateFinding),
    ("Localizer", Localizer),
    ("LocallyManufacturedProduct", LocallyManufacturedProduct),
    ("LocationInChest", LocationInChest),
    ("LocationOfSamplingSite", LocationOfSamplingSite),
    ("LocationOfSamplingSiteXOffset", LocationOfSamplingSiteXOffset),
    ("LocationOfSamplingSiteYOffset", LocationOfSamplingSiteYOffset),
    ("LocationOfSamplingSiteZOffset", LocationOfSamplingSiteZOffset),
    ("LocationOfSpecimen", LocationOfSpecimen),
    ("LocationOfSpecimenXOffset", LocationOfSpecimenXOffset),
    ("LocationOfSpecimenYOffset", LocationOfSpecimenYOffset),
    ("LocationOfSpecimenZOffset", LocationOfSpecimenZOffset),
    ("LogOfRatioOfTwoSamples", LogOfRatioOfTwoSamples),
    ("Login", Login),
    ("Logout", Logout),
    ("LongAxisBased", LongAxisBased),
    ("LongRunHighGrayLevelEmphasis", LongRunHighGrayLevelEmphasis),
    ("LongRunLowGrayLevelEmphasis", LongRunLowGrayLevelEmphasis),
    ("LongRunsEmphasis", LongRunsEmphasis),
    ("LongitudinalLagrangianStrain", LongitudinalLagrangianStrain),
    ("LongitudinalLagrangianStrainRate", LongitudinalLagrangianStrainRate),
    ("LongitudinalModulation", LongitudinalModulation),
    ("LongitudinalPositionZ", LongitudinalPositionZ),
    ("LongitudinalTemporalEventType", LongitudinalTemporalEventType),
    ("LongitudinalTemporalOffsetFromEvent", LongitudinalTemporalOffsetFromEvent),
    ("LossyCompressedImage", LossyCompressedImage),
    ("LossyCompressedPredecessor", LossyCompressedPredecessor),
    ("LossyCompression", LossyCompression),
    ("LotIdentifier", LotIdentifier),
    ("Low", Low),
    ("LowGrayLevelRunEmphasis", LowGrayLevelRunEmphasis),
    ("LowGrayLevelZoneEmphasis", LowGrayLevelZoneEmphasis),
    ("LowThyroidUptake", LowThyroidUptake),
    ("Lucency", Lucency),
    ("LumenDiameterRatio", LumenDiameterRatio),
    ("LumenEccentricityIndex", LumenEccentricityIndex),
    ("LumenPerimeter", LumenPerimeter),
    ("LumenShapeIndex", LumenShapeIndex),
    ("LumenVolume", LumenVolume),
    ("LuminanceResponseEvaluation", LuminanceResponseEvaluation),
    ("LuminanceUniformityEvaluation", LuminanceUniformityEvaluation),
    ("Lutetium177NAcetylaspartylglutamate", Lutetium177NAcetylaspartylglutamate),
    ("LympathicVesselInvasion", LympathicVesselInvasion),
    ("Lymphoma", Lymphoma),
    ("MDerivedFilter", MDerivedFilter),
    ("MEDI55189Zr", MEDI55189Zr),
    ("MEMSBasedTransducer", MEMSBasedTransducer),
    ("MIRDOSE", MIRDOSE),
    ("MIRDPamphlet1", MIRDPamphlet1),
    ("MLCPositionInterlock", MLCPositionInterlock),
    ("MLCPositionToleranceViolation", MLCPositionToleranceViolation),
    ("MLOEvidenceOfMotionBlur", MLOEvidenceOfMotionBlur),
    ("MLOInframammaryFoldIsNotOpen", MLOInframammaryFoldIsNotOpen),
    ("MLOInsufficientPectoralMuscle", MLOInsufficientPectoralMuscle),
    ("MLONoFatIsVisualizedPosteriorToFibroglandularTissues", MLONoFatIsVisualizedPosteriorToFibroglandularTissues),
    ("MLOPoorSeparationOfDeepAndSuperficialBreastTissues", MLOPoorSeparationOfDeepAndSuperficialBreastTissues),
    ("MLlerMethodPlanningForHipReplacement", MLlerMethodPlanningForHipReplacement),
    ("MODImported", MODImported),
    ("MPPSContent", MPPSContent),
    ("MRImageReconstruction", MRImageReconstruction),
    ("MRMarker", MRMarker),
    ("MRPerfusionPeak", MRPerfusionPeak),
    ("MRPerfusionSlope", MRPerfusionSlope),
    ("MRPerfusionTimeIntegral", MRPerfusionTimeIntegral),
    ("MRSignalIntensity", MRSignalIntensity),
    ("MVAEqualsFlowPer380SqrtGradientMmHg", MVAEqualsFlowPer380SqrtGradientMmHg),
    ("MachineCalibrationAdjustment", MachineCalibrationAdjustment),
    ("MachineCapabilityLicenseExpired", MachineCapabilityLicenseExpired),
    ("MachineClearanceInterlock", MachineClearanceInterlock),
    ("MachineNotAvailable", MachineNotAvailable),
    ("MachineQualityAssuranceInputUsed", MachineQualityAssuranceInputUsed),
    ("Macrocalcifications", Macrocalcifications),
    ("MaculaCentered", MaculaCentered),
    ("MacularFixationTesting", MacularFixationTesting),
    ("MacularGridThicknessAndVolumeReport", MacularGridThicknessAndVolumeReport),
    ("MacularThicknessKeyMeasurements", MacularThicknessKeyMeasurements),
    ("MagneticFieldStrength", MagneticFieldStrength),
    ("MagneticResonance", MagneticResonance),
    ("MagneticResonanceAngiography", MagneticResonanceAngiography),
    ("MagneticResonanceSpectroscopy", MagneticResonanceSpectroscopy),
    ("MagneticSusceptibility", MagneticSusceptibility),
    ("MagnetizationTransferRatio", MagnetizationTransferRatio),
    ("MagnificationSelection", MagnificationSelection),
    ("MagnitudeNormalized", MagnitudeNormalized),
    ("Male", Male),
    ("MaleChangedToFemale", MaleChangedToFemale),
    ("MalePseudohermaphrodite", MalePseudohermaphrodite),
    ("MaleTypical", MaleTypical),
    ("MalignancyType", MalignancyType),
    ("MalignantFibrousHistiocytoma", MalignantFibrousHistiocytoma),
    ("MalignantMelanomaOfNipple", MalignantMelanomaOfNipple),
    ("MammographicCrosshair", MammographicCrosshair),
    ("MammographicGrid", MammographicGrid),
    ("Mammography", Mammography),
    ("MammographyCAD", MammographyCAD),
    ("MammographyCADReport", MammographyCADReport),
    ("MammographyQualityControlManual1999ACR", MammographyQualityControlManual1999ACR),
    ("Manifest", Manifest),
    ("ManifestWithDescription", ManifestWithDescription),
    ("ManualAdministration", ManualAdministration),
    ("ManualEntry", ManualEntry),
    ("ManualHandlingInterval", ManualHandlingInterval),
    ("ManualHoldStarted", ManualHoldStarted),
    ("ManualKeratometry", ManualKeratometry),
    ("ManualProcessing", ManualProcessing),
    ("ManualResumeFromHold", ManualResumeFromHold),
    ("ManuallyTriggeredInjectionInformation", ManuallyTriggeredInjectionInformation),
    ("ManufacturerImplantTemplate", ManufacturerImplantTemplate),
    ("Margetuximab89Zr", Margetuximab89Zr),
    ("Margins", Margins),
    ("MarkerPlacement", MarkerPlacement),
    ("MaskImageForImageProcessingOperation", MaskImageForImageProcessingOperation),
    ("MassAdministered", MassAdministered),
    ("MassInTheSkin", MassInTheSkin),
    ("MassOccupancyImage", MassOccupancyImage),
    ("MassOfBreast", MassOfBreast),
    ("MassOfParenchymalTissue", MassOfParenchymalTissue),
    ("MassOfVascularCalcification", MassOfVascularCalcification),
    ("MassOnTheSkin", MassOnTheSkin),
    ("MassScoringMethod", MassScoringMethod),
    ("MassWithCalcifications", MassWithCalcifications),
    ("MaterialHighlightedImage", MaterialHighlightedImage),
    ("MaterialRecalculatedImage", MaterialRecalculatedImage),
    ("MaterialRemovedImage", MaterialRemovedImage),
    ("MaterialSpecificImage", MaterialSpecificImage),
    ("MaterialSuppressedImage", MaterialSuppressedImage),
    ("Maternal", Maternal),
    ("MatingFeatureID", MatingFeatureID),
    ("MatingFeatureSetID", MatingFeatureSetID),
    ("MaxDpPerDt", MaxDpPerDt),
    ("MaxDpPerDtPerP", MaxDpPerDtPerP),
    ("MaxNegDpPerDt", MaxNegDpPerDt),
    ("MaxVolumeNormalizedTo50mmHgPulsePressure", MaxVolumeNormalizedTo50mmHgPulsePressure),
    ("MaximumAbsoluteVolumeAtRadiationDose", MaximumAbsoluteVolumeAtRadiationDose),
    ("MaximumAbsorbedRadiationDose", MaximumAbsorbedRadiationDose),
    ("MaximumAorticPlaqueThickness", MaximumAorticPlaqueThickness),
    ("MaximumAttenuationCoefficient", MaximumAttenuationCoefficient),
    ("MaximumCADOperatingPoint", MaximumCADOperatingPoint),
    ("MaximumDifference", MaximumDifference),
    ("MaximumEquivalentRadiationDose", MaximumEquivalentRadiationDose),
    ("MaximumEquivalentUniformDose", MaximumEquivalentUniformDose),
    ("MaximumHomogeneityIndex", MaximumHomogeneityIndex),
    ("MaximumIntensityProjection", MaximumIntensityProjection),
    ("MaximumMeanRadiationDose", MaximumMeanRadiationDose),
    ("MaximumOrificeArea", MaximumOrificeArea),
    ("MaximumPercentVolumeAtRadiationDose", MaximumPercentVolumeAtRadiationDose),
    ("MaximumPowerOutputAchieved", MaximumPowerOutputAchieved),
    ("MaximumPressureAcceleration", MaximumPressureAcceleration),
    ("MaximumRadiationDose", MaximumRadiationDose),
    ("MaximumSlope", MaximumSlope),
    ("MaximumSurfaceRadiationDose", MaximumSurfaceRadiationDose),
    ("MaximumXRayTubeCurrent", MaximumXRayTubeCurrent),
    ("MeanAbsorbedRadiationDose", MeanAbsorbedRadiationDose),
    ("MeanAttenuationCoefficient", MeanAttenuationCoefficient),
    ("MeanBloodPressure", MeanBloodPressure),
    ("MeanCTDIfreeair", MeanCTDIfreeair),
    ("MeanCTDIvol", MeanCTDIvol),
    ("MeanDiffusivity", MeanDiffusivity),
    ("MeanEquivalentRadiationDose", MeanEquivalentRadiationDose),
    ("MeanIntensityProjection", MeanIntensityProjection),
    ("MeanKurtosis", MeanKurtosis),
    ("MeanLocalReference", MeanLocalReference),
    ("MeanTransitTime", MeanTransitTime),
    ("MeanUltrasoundAttenuationCoefficient", MeanUltrasoundAttenuationCoefficient),
    ("MeanValueChosen", MeanValueChosen),
    ("MeasuredAPDimension", MeasuredAPDimension),
    ("MeasuredLateralDimension", MeasuredLateralDimension),
    ("MeasuredProperty", MeasuredProperty),
    ("MeasuredRadiationDose", MeasuredRadiationDose),
    ("MeasurementDivisor", MeasurementDivisor),
    ("MeasurementFailure", MeasurementFailure),
    ("MeasurementFromThisDevice", MeasurementFromThisDevice),
    ("MeasurementGroup", MeasurementGroup),
    ("MeasurementNotAttempted", MeasurementNotAttempted),
    ("MeasurementOfResponse", MeasurementOfResponse),
    ("MeasurementOrientation", MeasurementOrientation),
    ("MeasurementType", MeasurementType),
    ("MechanicalBeamSteering", MechanicalBeamSteering),
    ("MechanicalDispersion", MechanicalDispersion),
    ("MechanicalFailure", MechanicalFailure),
    ("MechanicalPointer", MechanicalPointer),
    ("MediaCreationDevice", MediaCreationDevice),
    ("MediaFailure", MediaFailure),
    ("MediaImport", MediaImport),
    ("MedialDissection", MedialDissection),
    ("Median", Median),
    ("MedianAbsorbedRadiationDose", MedianAbsorbedRadiationDose),
    ("MedianAttenuationCoefficient", MedianAttenuationCoefficient),
    ("MedianEquivalentRadiationDose", MedianEquivalentRadiationDose),
    ("MedianIntensityProjection", MedianIntensityProjection),
    ("MedianUltrasoundAttenuationCoefficient", MedianUltrasoundAttenuationCoefficient),
    ("MedicalDeviceUsed", MedicalDeviceUsed),
    ("MedicalPhysics", MedicalPhysics),
    ("MedicationHistory", MedicationHistory),
    ("MedicationType", MedicationType),
    ("MedicationsSet", MedicationsSet),
    ("Medium", Medium),
    ("Medium320380CmLateralThickness", Medium320380CmLateralThickness),
    ("MenopausalPhase", MenopausalPhase),
    ("MenstrualCyclePhase", MenstrualCyclePhase),
    ("MeshRadiationTransportModel", MeshRadiationTransportModel),
    ("MesopicLight", MesopicLight),
    ("MessagingSystem", MessagingSystem),
    ("MetabolicVolume", MetabolicVolume),
    ("MetaboliteMapsFromSpectroscopyData", MetaboliteMapsFromSpectroscopyData),
    ("MetastasisToAnIntramammaryLymphNode", MetastasisToAnIntramammaryLymphNode),
    ("MetastaticCancerToTheBreast", MetastaticCancerToTheBreast),
    ("MetastaticCancerToTheBreastFromTheColon", MetastaticCancerToTheBreastFromTheColon),
    ("MetastaticCancerToTheBreastFromTheLung", MetastaticCancerToTheBreastFromTheLung),
    ("MetastaticCancerToTheBreastFromTheOvary", MetastaticCancerToTheBreastFromTheOvary),
    ("MetastaticDiseaseToAxillaryNode", MetastaticDiseaseToAxillaryNode),
    ("MetastaticMelanomaToTheBreast", MetastaticMelanomaToTheBreast),
    ("MetastaticSarcomaToTheBreast", MetastaticSarcomaToTheBreast),
    ("Meterset", Meterset),
    ("MetersetRateInterlock", MetersetRateInterlock),
    ("MetersetToleranceViolation", MetersetToleranceViolation),
    ("MethodCitation", MethodCitation),
    ("MethodOfDisksBiplane", MethodOfDisksBiplane),
    ("MethodOfDisksSinglePlane", MethodOfDisksSinglePlane),
    ("MicroclipPlaced", MicroclipPlaced),
    ("MicrodosimetricKineticModel", MicrodosimetricKineticModel),
    ("MicroglandularAdenosis", MicroglandularAdenosis),
    ("Micronodule", Micronodule),
    ("MidEsophageal0DegreeTEE", MidEsophageal0DegreeTEE),
    ("MidEsophageal135DegreeTEE", MidEsophageal135DegreeTEE),
    ("MidEsophageal45DegreeTEE", MidEsophageal45DegreeTEE),
    ("MidEsophageal60DegreeTEE", MidEsophageal60DegreeTEE),
    ("MidEsophageal90DegreeTEE", MidEsophageal90DegreeTEE),
    ("MidPeripheralInferior", MidPeripheralInferior),
    ("MidPeripheralInferiorNasal", MidPeripheralInferiorNasal),
    ("MidPeripheralInferiorTemporal", MidPeripheralInferiorTemporal),
    ("MidPeripheralNasal", MidPeripheralNasal),
    ("MidPeripheralSuperior", MidPeripheralSuperior),
    ("MidPeripheralSuperiorNasal", MidPeripheralSuperiorNasal),
    ("MidPeripheralSuperiorTemporal", MidPeripheralSuperiorTemporal),
    ("MidPeripheralTemporal", MidPeripheralTemporal),
    ("MiddleInnerRetinaVasculatureFlow", MiddleInnerRetinaVasculatureFlow),
    ("MiddleInnerStructuralReflectanceMap", MiddleInnerStructuralReflectanceMap),
    ("MiddleOfTimePeriod", MiddleOfTimePeriod),
    ("MidlineOfTheEllipsoidZone", MidlineOfTheEllipsoidZone),
    ("MidlungWindow", MidlungWindow),
    ("MidsagittalFacialProfile", MidsagittalFacialProfile),
    ("MidsagittalView", MidsagittalView),
    ("MiliaryPattern", MiliaryPattern),
    ("MinimizeMeterset", MinimizeMeterset),
    ("MinimumAbsoluteVolumeAtRadiationDose", MinimumAbsoluteVolumeAtRadiationDose),
    ("MinimumAbsorbedRadiationDose", MinimumAbsorbedRadiationDose),
    ("MinimumAttenuationCoefficient", MinimumAttenuationCoefficient),
    ("MinimumConformationNumber", MinimumConformationNumber),
    ("MinimumConformityIndex", MinimumConformityIndex),
    ("MinimumEquivalentRadiationDose", MinimumEquivalentRadiationDose),
    ("MinimumEquivalentUniformDose", MinimumEquivalentUniformDose),
    ("MinimumGanglionCellThickness", MinimumGanglionCellThickness),
    ("MinimumHealthyTissueConformityIndex", MinimumHealthyTissueConformityIndex),
    ("MinimumIntensityProjection", MinimumIntensityProjection),
    ("MinimumMeanRadiationDose", MinimumMeanRadiationDose),
    ("MinimumPercentVolumeAtRadiationDose", MinimumPercentVolumeAtRadiationDose),
    ("MinimumRadiationDose", MinimumRadiationDose),
    ("MinimumSurfaceRadiationDose", MinimumSurfaceRadiationDose),
    ("MitralAnnularExcursion", MitralAnnularExcursion),
    ("MitralAnnulusAnterolateralToPosteromedialDiameter", MitralAnnulusAnterolateralToPosteromedialDiameter),
    ("MitralAnnulusAnteroposteriorDiameter", MitralAnnulusAnteroposteriorDiameter),
    ("MitralAnnulusArea", MitralAnnulusArea),
    ("MitralAnnulusCalcificationSeverity", MitralAnnulusCalcificationSeverity),
    ("MitralAnnulusCommissuralDiameter", MitralAnnulusCommissuralDiameter),
    ("MitralAnnulusDiameterRatio", MitralAnnulusDiameterRatio),
    ("MitralAnnulusHeight", MitralAnnulusHeight),
    ("MitralAnnulusNonplanarityAngle", MitralAnnulusNonplanarityAngle),
    ("MitralAnnulusPerimeter", MitralAnnulusPerimeter),
    ("MitralAnteriorLeafletA1ScallopLength", MitralAnteriorLeafletA1ScallopLength),
    ("MitralAnteriorLeafletA2ScallopLength", MitralAnteriorLeafletA2ScallopLength),
    ("MitralAnteriorLeafletA3ScallopLength", MitralAnteriorLeafletA3ScallopLength),
    ("MitralAnteriorLeafletArea", MitralAnteriorLeafletArea),
    ("MitralCommissureDistance", MitralCommissureDistance),
    ("MitralTrigoneToTrigoneDistance", MitralTrigoneToTrigoneDistance),
    ("MitralValveAnnulusDiameter", MitralValveAnnulusDiameter),
    ("MitralValveClosure", MitralValveClosure),
    ("MitralValveCoaptationLength", MitralValveCoaptationLength),
    ("MitralValveInterpapillaryDistance", MitralValveInterpapillaryDistance),
    ("MitralValveOpening", MitralValveOpening),
    ("MitralValveProlapseArea", MitralValveProlapseArea),
    ("MitralValveProlapseVolume", MitralValveProlapseVolume),
    ("MitralValveSegmentFlailGap", MitralValveSegmentFlailGap),
    ("MitralValveSphericityIndex", MitralValveSphericityIndex),
    ("MitralValveTentingArea", MitralValveTentingArea),
    ("MitralValveTentingHeight", MitralValveTentingHeight),
    ("MitralValveTentingSegmentHeightA1P1", MitralValveTentingSegmentHeightA1P1),
    ("MitralValveTentingSegmentHeightA2P2", MitralValveTentingSegmentHeightA2P2),
    ("MitralValveTentingSegmentHeightA3P3", MitralValveTentingSegmentHeightA3P3),
    ("MixedModality3DCAMModel", MixedModality3DCAMModel),
    ("MixedSex", MixedSex),
    ("Modality", Modality),
    ("ModalityToRead", ModalityToRead),
    ("ModeAbsorbedRadiationDose", ModeAbsorbedRadiationDose),
    ("ModeEquivalentRadiationDose", ModeEquivalentRadiationDose),
    ("ModelFittingMethod", ModelFittingMethod),
    ("ModelFree", ModelFree),
    ("ModelFreeConcentrationTimeQuantitification", ModelFreeConcentrationTimeQuantitification),
    ("ModelMaximumAge", ModelMaximumAge),
    ("ModelMaximumHeight", ModelMaximumHeight),
    ("ModelMaximumWeight", ModelMaximumWeight),
    ("ModelMinimumAge", ModelMinimumAge),
    ("ModelMinimumHeight", ModelMinimumHeight),
    ("ModelMinimumWeight", ModelMinimumWeight),
    ("ModelPatientSex", ModelPatientSex),
    ("ModifiedHounsfieldUnit", ModifiedHounsfieldUnit),
    ("ModifiedSimpson", ModifiedSimpson),
    ("ModifyingEquipment", ModifyingEquipment),
    ("Mogamulizumab89Zr", Mogamulizumab89Zr),
    ("Mold", Mold),
    ("MonoExponentialApparentDiffusionCoefficient", MonoExponentialApparentDiffusionCoefficient),
    ("MonoExponentialDiffusionModel", MonoExponentialDiffusionModel),
    ("MonoblockStem", MonoblockStem),
    ("MonoclonalAntibody64Cu", MonoclonalAntibody64Cu),
    ("MonoclonalAntibody89Zr", MonoclonalAntibody89Zr),
    ("MonopoleSignal", MonopoleSignal),
    ("MorphologicalOperations", MorphologicalOperations),
    ("MosaicPattern", MosaicPattern),
    ("MosaicPerfusion", MosaicPerfusion),
    ("MostRecentValueChosen", MostRecentValueChosen),
    ("MotherOfFetus", MotherOfFetus),
    ("MotionBlur", MotionBlur),
    ("MotorizedWedge", MotorizedWedge),
    ("MouseAlphaSynucleinPreformedFibrils", MouseAlphaSynucleinPreformedFibrils),
    ("MouseTauPreformedFibrils", MouseTauPreformedFibrils),
    ("MoveableLaserPatientSetupPoint", MoveableLaserPatientSetupPoint),
    ("MultiEnergyMAR", MultiEnergyMAR),
    ("MultiEnergyProportionalWeighting", MultiEnergyProportionalWeighting),
    ("MultiMediaCard", MultiMediaCard),
    ("MultiScalePerResolutionFiltering", MultiScalePerResolutionFiltering),
    ("MultiSideIllumination", MultiSideIllumination),
    ("MultiTensor", MultiTensor),
    ("MultifocalIntraductalCarcinoma", MultifocalIntraductalCarcinoma),
    ("MultifocalInvasiveDuctalCarcinoma", MultifocalInvasiveDuctalCarcinoma),
    ("MultiparametricMRI", MultiparametricMRI),
    ("MultiparametricMRIOfProstate", MultiparametricMRIOfProstate),
    ("MultiparametricMRIOfWholeBody", MultiparametricMRIOfWholeBody),
    ("MultiplanarReformatting", MultiplanarReformatting),
    ("MultipleFillingDefect", MultipleFillingDefect),
    ("MultipleFixedSources", MultipleFixedSources),
    ("MultipleIntraductalPapillomas", MultipleIntraductalPapillomas),
    ("MultipleLADRegionInRAOProjection", MultipleLADRegionInRAOProjection),
    ("MultiplePlanes", MultiplePlanes),
    ("MultipleRCARegionInRAOProjection", MultipleRCARegionInRAOProjection),
    ("MultipleShotEPI", MultipleShotEPI),
    ("MultipleSlices", MultipleSlices),
    ("MultiplexGroupNumber", MultiplexGroupNumber),
    ("MultiplexGroupUID", MultiplexGroupUID),
    ("MultispectralProcessing", MultispectralProcessing),
    ("MusculoskeletalImagingSpecialty", MusculoskeletalImagingSpecialty),
    ("MusculoskeletalImagingSubjectMatter", MusculoskeletalImagingSubjectMatter),
    ("MusculoskeletalRadiology", MusculoskeletalRadiology),
    ("MyocardialBiopsy", MyocardialBiopsy),
    ("MyocardialMidlayerMethod", MyocardialMidlayerMethod),
    ("MyocardialPerfusionAnalysis", MyocardialPerfusionAnalysis),
    ("Myofibroblastoma", Myofibroblastoma),
    ("N0DocumentedAbsenceOfCAD", N0DocumentedAbsenceOfCAD),
    ("N100MmPediatricHeadCTDosimetryPhantom", N100MmPediatricHeadCTDosimetryPhantom),
    ("N100cmFromXRaySource", N100cmFromXRaySource),
    ("N10X", N10X),
    ("N10YearCHDRisk", N10YearCHDRisk),
    ("N12CarbonNucleus", N12CarbonNucleus),
    ("N152Terbium", N152Terbium),
    ("N15LeadECG", N15LeadECG),
    ("N15cmFromIsocenterTowardSource", N15cmFromIsocenterTowardSource),
    ("N15cmFromTableCenterline", N15cmFromTableCenterline),
    ("N16OxygenNucleus", N16OxygenNucleus),
    ("N191mIridium", N191mIridium),
    ("N1MinimalNonObstructiveCAD", N1MinimalNonObstructiveCAD),
    ("N1cmAboveTabletop", N1cmAboveTabletop),
    ("N1stWeek", N1stWeek),
    ("N20X", N20X),
    ("N28H189Zr", N28H189Zr),
    ("N2DShearWaveElastography", N2DShearWaveElastography),
    ("N2DSpeckleTracking", N2DSpeckleTracking),
    ("N2FAF18", N2FAF18),
    ("N2MildNonObstructiveCAD", N2MildNonObstructiveCAD),
    ("N2SigmaDeviationOfPopulation", N2SigmaDeviationOfPopulation),
    ("N2ndWeek", N2ndWeek),
    ("N30cmAboveTabletop", N30cmAboveTabletop),
    ("N30cmInFrontOfImageInputSurface", N30cmInFrontOfImageInputSurface),
    ("N35PercentOfThermalPerDyeDilutionCO", N35PercentOfThermalPerDyeDilutionCO),
    ("N3DDoseMap", N3DDoseMap),
    ("N3DGel", N3DGel),
    ("N3DManufacturingModelingSystem", N3DManufacturingModelingSystem),
    ("N3DMode", N3DMode),
    ("N3DShearWaveElastography", N3DShearWaveElastography),
    ("N3DSpeckleTracking", N3DSpeckleTracking),
    ("N3HeliumNucleus", N3HeliumNucleus),
    ("N3ModerateStenosis", N3ModerateStenosis),
    ("N3rdWeek", N3rdWeek),
    ("N40X", N40X),
    ("N42cmAboveBreastSupportSurface", N42cmAboveBreastSupportSurface),
    ("N43Scandium", N43Scandium),
    ("N44Scandium", N44Scandium),
    ("N4ASevereStenosis", N4ASevereStenosis),
    ("N4BSevereStenosis", N4BSevereStenosis),
    ("N4DRadiationTreatmentNote", N4DRadiationTreatmentNote),
    ("N4HeliumNucleus", N4HeliumNucleus),
    ("N4MonthsTo1YearAgo", N4MonthsTo1YearAgo),
    ("N4PointSegmentFindingScale", N4PointSegmentFindingScale),
    ("N4SevereStenosis", N4SevereStenosis),
    ("N51Manganese", N51Manganese),
    ("N52mManganese", N52mManganese),
    ("N5PointSegmentFindingScale", N5PointSegmentFindingScale),
    ("N5PointSegmentFindingScaleWithGradedHypokinesis", N5PointSegmentFindingScaleWithGradedHypokinesis),
    ("N5TotalCoronaryOcclusion", N5TotalCoronaryOcclusion),
    ("N5X", N5X),
    ("N70Arsenic", N70Arsenic),
    ("N70PercentOfThermalPerDyeDilutionCO", N70PercentOfThermalPerDyeDilutionCO),
    ("N7D1289Zr", N7D1289Zr),
    ("N7E1189Zr", N7E1189Zr),
    ("N90Niobium", N90Niobium),
    ("NASCET", NASCET),
    ("NAcetylaspartatePerCholineRatio", NAcetylaspartatePerCholineRatio),
    ("NAcetylaspartatePerCreatineRatio", NAcetylaspartatePerCreatineRatio),
    ("NEMAXR212000Phantom", NEMAXR212000Phantom),
    ("NIH07", NIH07),
    ("NIH31", NIH31),
    ("NMMyocardialViabilityProcedure", NMMyocardialViabilityProcedure),
    ("NMTransmissionAttenuationCorrection", NMTransmissionAttenuationCorrection),
    ("NNonDiagnosticSegments", NNonDiagnosticSegments),
    ("NNonDiagnosticStudy", NNonDiagnosticStudy),
    ("NURBSRadiationTransportModel", NURBSRadiationTransportModel),
    ("NanocolloidalAlbumin89Zr", NanocolloidalAlbumin89Zr),
    ("NarrativeSummary", NarrativeSummary),
    ("NasalCannula", NasalCannula),
    ("NationalKennelClub", NationalKennelClub),
    ("NativePlaqueVolume", NativePlaqueVolume),
    ("NearPupillaryDistance", NearPupillaryDistance),
    ("Nearline", Nearline),
    ("NeckComponent", NeckComponent),
    ("NecroticLipidicPlaque", NecroticLipidicPlaque),
    ("NeedleGauge", NeedleGauge),
    ("NeedleInTarget", NeedleInTarget),
    ("NeedleLength", NeedleLength),
    ("NeedleLocalizationAndBiopsy", NeedleLocalizationAndBiopsy),
    ("NegativeDCE", NegativeDCE),
    ("NegativeEnhancementIntegral", NegativeEnhancementIntegral),
    ("NegativeExponential", NegativeExponential),
    ("NegativeInfinity", NegativeInfinity),
    ("NeighborhoodAnalysis", NeighborhoodAnalysis),
    ("NeighbourhoodGreyToneDifferenceMatrix", NeighbourhoodGreyToneDifferenceMatrix),
    ("NeighbouringGreyLevelDependenceMatrix", NeighbouringGreyLevelDependenceMatrix),
    ("NeoplasmOfTheMammarySkin", NeoplasmOfTheMammarySkin),
    ("NetForwardVolume", NetForwardVolume),
    ("NetworkConfiguration", NetworkConfiguration),
    ("NetworkEntry", NetworkEntry),
    ("Neurofibromatosis", Neurofibromatosis),
    ("NeuroimagingSubjectMatter", NeuroimagingSubjectMatter),
    ("NeurologySpecialty", NeurologySpecialty),
    ("NeurophysiologyAutomatedAnalysisAnnotations", NeurophysiologyAutomatedAnalysisAnnotations),
    ("NeurophysiologyPostHocReviewAnnotations", NeurophysiologyPostHocReviewAnnotations),
    ("NeurophysiologyRecordingAnnotations", NeurophysiologyRecordingAnnotations),
    ("NeuroradiologyImagingSpecialty", NeuroradiologyImagingSpecialty),
    ("NeuroretinalRimArea", NeuroretinalRimArea),
    ("NeuroretinalRimWidth", NeuroretinalRimWidth),
    ("NeutralMusculoskeletalPosition", NeutralMusculoskeletalPosition),
    ("NifeneF18", NifeneF18),
    ("NilPerOsNPOStatusConfirmed", NilPerOsNPOStatusConfirmed),
    ("NiobiumOrNiobiumCompound", NiobiumOrNiobiumCompound),
    ("NippleCharacteristic", NippleCharacteristic),
    ("NippleDischargeCytology", NippleDischargeCytology),
    ("NippleInvolved", NippleInvolved),
    ("NippleNotInProfile", NippleNotInProfile),
    ("NippleRing", NippleRing),
    ("NoAbnormality", NoAbnormality),
    ("NoAlgorithmsSucceededWithoutFindings", NoAlgorithmsSucceededWithoutFindings),
    ("NoAttenuationCorrection", NoAttenuationCorrection),
    ("NoComplications", NoComplications),
    ("NoCornealCompensation", NoCornealCompensation),
    ("NoCorrelationToClinicalFindings", NoCorrelationToClinicalFindings),
    ("NoCorrelationToOtherImagingFindings", NoCorrelationToOtherImagingFindings),
    ("NoEmptyTileSuppression", NoEmptyTileSuppression),
    ("NoFamilyHistoryOfProstateCancer", NoFamilyHistoryOfProstateCancer),
    ("NoFilter", NoFilter),
    ("NoGrid", NoGrid),
    ("NoImage", NoImage),
    ("NoKnownExposure", NoKnownExposure),
    ("NoModulation", NoModulation),
    ("NoPosteriorAcousticFeatures", NoPosteriorAcousticFeatures),
    ("NoRealignment", NoRealignment),
    ("NoSensitiveContentIdentified", NoSensitiveContentIdentified),
    ("NoSubsequentWorkitems", NoSubsequentWorkitems),
    ("NodeAuthentication", NodeAuthentication),
    ("NodeID", NodeID),
    ("NodularPattern", NodularPattern),
    ("NoiseIndex", NoiseIndex),
    ("Nomenclature", Nomenclature),
    ("NominalEmptyTileSuppression", NominalEmptyTileSuppression),
    ("NominalImagingSourceLocation", NominalImagingSourceLocation),
    ("NominalRadiationSourceLocation", NominalRadiationSourceLocation),
    ("NominalSingleCollimationWidth", NominalSingleCollimationWidth),
    ("NominalTotalCollimationWidth", NominalTotalCollimationWidth),
    ("NonBloodyDischargeFromNipple", NonBloodyDischargeFromNipple),
    ("NonDiagnosticECG", NonDiagnosticECG),
    ("NonDiagnosticLowHeartRate", NonDiagnosticLowHeartRate),
    ("NonDiagnosticRestingSTAbnormalities", NonDiagnosticRestingSTAbnormalities),
    ("NonDiagnosticVentricularPacingOrLBBB", NonDiagnosticVentricularPacingOrLBBB),
    ("NonFlatteningFilterBeam", NonFlatteningFilterBeam),
    ("NonFocalAbnormality", NonFocalAbnormality),
    ("NonImagingDopplerUltrasoundTransducerGeometry", NonImagingDopplerUltrasoundTransducerGeometry),
    ("NonIonicIodinatedContrastAgent", NonIonicIodinatedContrastAgent),
    ("NonLesion", NonLesion),
    ("NonLesionAtBaseline", NonLesionAtBaseline),
    ("NonLesionModifier", NonLesionModifier),
    ("NonSpecificVolume", NonSpecificVolume),
    ("NonSynchronizedRoboticTreatment", NonSynchronizedRoboticTreatment),
    ("NonTargetLesionAtBaseline", NonTargetLesionAtBaseline),
    ("NonTargetLesionCompleteResponse", NonTargetLesionCompleteResponse),
    ("NonTargetLesionIncompleteResponseOrStableDisease", NonTargetLesionIncompleteResponseOrStableDisease),
    ("NonTargetLesionProgressiveDisease", NonTargetLesionProgressiveDisease),
    ("NonUterineLeiomyosarcoma", NonUterineLeiomyosarcoma),
    ("NormalAxillaryNode", NormalAxillaryNode),
    ("NormalBreastTissue", NormalBreastTissue),
    ("NormalImplants", NormalImplants),
    ("NormalIntervalFollowUp", NormalIntervalFollowUp),
    ("NormalMyocardium", NormalMyocardium),
    ("NormalRangeAuthority", NormalRangeAuthority),
    ("NormalRangeDescription", NormalRangeDescription),
    ("Normality", Normality),
    ("NormalizationFactor", NormalizationFactor),
    ("NormalizedChordLength", NormalizedChordLength),
    ("NormalizedValuesOfVentricularMeasurements", NormalizedValuesOfVentricularMeasurements),
    ("NorthAmericanPurebredDogRegistry", NorthAmericanPurebredDogRegistry),
    ("NoseCone", NoseCone),
    ("NotANumber", NotANumber),
    ("NotAllAlgorithmsSucceededWithFindings", NotAllAlgorithmsSucceededWithFindings),
    ("NotAllAlgorithmsSucceededWithoutFindings", NotAllAlgorithmsSucceededWithoutFindings),
    ("NotAttempted", NotAttempted),
    ("NotForPresentationRenderingDeviceExpectedNotToPresent", NotForPresentationRenderingDeviceExpectedNotToPresent),
    ("NotOptimizedForTheDeviceInstance", NotOptimizedForTheDeviceInstance),
    ("NotParallel", NotParallel),
    ("NotVisualized", NotVisualized),
    ("NuclearMedicine", NuclearMedicine),
    ("NuclearMedicineImagingSubjectMatter", NuclearMedicineImagingSubjectMatter),
    ("NuclearMedicineProjectionActivity", NuclearMedicineProjectionActivity),
    ("NuclearMedicineTomographicActivity", NuclearMedicineTomographicActivity),
    ("NudityContent", NudityContent),
    ("NumberOfAnimalsWithinSameHousingUnit", NumberOfAnimalsWithinSameHousingUnit),
    ("NumberOfCalcifications", NumberOfCalcifications),
    ("NumberOfChannels", NumberOfChannels),
    ("NumberOfDiseasedVesselTerritories", NumberOfDiseasedVesselTerritories),
    ("NumberOfEctopicBeats", NumberOfEctopicBeats),
    ("NumberOfFetuses", NumberOfFetuses),
    ("NumberOfFirstDegreeRelativesAffectedByMalignantMelanoma", NumberOfFirstDegreeRelativesAffectedByMalignantMelanoma),
    ("NumberOfFocalPlanes", NumberOfFocalPlanes),
    ("NumberOfFolliclesInRange", NumberOfFolliclesInRange),
    ("NumberOfFractionsCompleted", NumberOfFractionsCompleted),
    ("NumberOfFractionsPlanned", NumberOfFractionsPlanned),
    ("NumberOfFrames", NumberOfFrames),
    ("NumberOfHistories", NumberOfHistories),
    ("NumberOfHousingUnitsPerRack", NumberOfHousingUnitsPerRack),
    ("NumberOfImagesUsedForMacularMeasurements", NumberOfImagesUsedForMacularMeasurements),
    ("NumberOfInjectorHeads", NumberOfInjectorHeads),
    ("NumberOfLesionInterventionsAttempted", NumberOfLesionInterventionsAttempted),
    ("NumberOfLesionInterventionsSuccessful", NumberOfLesionInterventionsSuccessful),
    ("NumberOfMalignantMelanomas", NumberOfMalignantMelanomas),
    ("NumberOfMelanomasInSitu", NumberOfMelanomasInSitu),
    ("NumberOfNeedlesAroundTarget", NumberOfNeedlesAroundTarget),
    ("NumberOfNodesPositive", NumberOfNodesPositive),
    ("NumberOfNodesRemoved", NumberOfNodesRemoved),
    ("NumberOfPasses", NumberOfPasses),
    ("NumberOfPixels", NumberOfPixels),
    ("NumberOfPulses", NumberOfPulses),
    ("NumberOfRacksPerRoom", NumberOfRacksPerRoom),
    ("NumberOfSamplesUsedPerImage", NumberOfSamplesUsedPerImage),
    ("NumberOfSeriesRelatedInstances", NumberOfSeriesRelatedInstances),
    ("NumberOfSimilarFindings", NumberOfSimilarFindings),
    ("NumberOfSpecimens", NumberOfSpecimens),
    ("NumberOfStimulusEvents", NumberOfStimulusEvents),
    ("NumberOfStudyRelatedSeries", NumberOfStudyRelatedSeries),
    ("NumberOfVoxels", NumberOfVoxels),
    ("NumberOfXRaySources", NumberOfXRaySources),
    ("NumericalAcousticModel", NumericalAcousticModel),
    ("Nurse", Nurse),
    ("NursingNote", NursingNote),
    ("NursingUnitCancel", NursingUnitCancel),
    ("O10", O10),
    ("O9", O9),
    ("OBGYNUltrasoundProcedureReport", OBGYNUltrasoundProcedureReport),
    ("OBPerGynImagingSpecialty", OBPerGynImagingSpecialty),
    ("OCTAAmplitudeDecorrelation", OCTAAmplitudeDecorrelation),
    ("OCTAComplexVariance", OCTAComplexVariance),
    ("OCTACorrelationMapping", OCTACorrelationMapping),
    ("OCTAOneSidedRatioGreater", OCTAOneSidedRatioGreater),
    ("OCTAOneSidedRatioLesser", OCTAOneSidedRatioLesser),
    ("OCTASpeckleVariance", OCTASpeckleVariance),
    ("OCTBScanAnalysis", OCTBScanAnalysis),
    ("OIQPattern", OIQPattern),
    ("OLED", OLED),
    ("OLINDAEXM", OLINDAEXM),
    ("ORCCStructuralReflectanceMap", ORCCStructuralReflectanceMap),
    ("ORCCVasculatureFlow", ORCCVasculatureFlow),
    ("OSEMAlgorithm", OSEMAlgorithm),
    ("OSLD", OSLD),
    ("Obinituzimab89Zr", Obinituzimab89Zr),
    ("ObjectRemoved", ObjectRemoved),
    ("ObjectSecurityAttributesChanged", ObjectSecurityAttributesChanged),
    ("ObjectSetIncomplete", ObjectSetIncomplete),
    ("ObjectType", ObjectType),
    ("ObjectTypesNotSupported", ObjectTypesNotSupported),
    ("ObjectsIncorrectlyFormatted", ObjectsIncorrectlyFormatted),
    ("ObliqueIllumination", ObliqueIllumination),
    ("ObliqueShortAxisViewAtDuctusArteriosus", ObliqueShortAxisViewAtDuctusArteriosus),
    ("ObservationByExaminer", ObservationByExaminer),
    ("ObservationDateTimeQualifier", ObservationDateTimeQualifier),
    ("ObserverType", ObserverType),
    ("Ocaratuzumab89Zr", Ocaratuzumab89Zr),
    ("OccultBloodTestResult", OccultBloodTestResult),
    ("OccultCarcinomaPresentingWithAxillaryLymphNodeMetastases", OccultCarcinomaPresentingWithAxillaryLymphNodeMetastases),
    ("OcularFixationLight", OcularFixationLight),
    ("OcularGazeSetupMethod", OcularGazeSetupMethod),
    ("OfInterest", OfInterest),
    ("OfficeCalled", OfficeCalled),
    ("Offline", Offline),
    ("OffsetFactor", OffsetFactor),
    ("OilCystFatNecrosisCyst", OilCystFatNecrosisCyst),
    ("OldFilmsForComparison", OldFilmsForComparison),
    ("Oligemia", Oligemia),
    ("Olsen", Olsen),
    ("OnAdmissionToUnit", OnAdmissionToUnit),
    ("OnDischarge", OnDischarge),
    ("OnDischargeFromUnit", OnDischargeFromUnit),
    ("OncologicImagingSpecialty", OncologicImagingSpecialty),
    ("OncologyMeasurementReport", OncologyMeasurementReport),
    ("OncologySpecialty", OncologySpecialty),
    ("Ongoing", Ongoing),
    ("Online", Online),
    ("OnsetOfExercise", OnsetOfExercise),
    ("OnsetOfStimulation", OnsetOfStimulation),
    ("Opacity", Opacity),
    ("OpacityDescriptor", OpacityDescriptor),
    ("OperatorDecisionToTerminateTreatment", OperatorDecisionToTerminateTreatment),
    ("OperatorError", OperatorError),
    ("OperatorsNarrative", OperatorsNarrative),
    ("OphthalmicAxialMeasurements", OphthalmicAxialMeasurements),
    ("OphthalmicImageROIMeasurements", OphthalmicImageROIMeasurements),
    ("OphthalmicMacularGridProblem", OphthalmicMacularGridProblem),
    ("OphthalmicMapping", OphthalmicMapping),
    ("OphthalmicPhotography", OphthalmicPhotography),
    ("OphthalmicRefraction", OphthalmicRefraction),
    ("OphthalmicTomography", OphthalmicTomography),
    ("OphthalmicTomographyBScanVolumeAnalysis", OphthalmicTomographyBScanVolumeAnalysis),
    ("OphthalmicTomographyEnFace", OphthalmicTomographyEnFace),
    ("OphthalmicVisualField", OphthalmicVisualField),
    ("OppositeRegionSeverity", OppositeRegionSeverity),
    ("OpticCupArea", OpticCupArea),
    ("OpticCupVolume", OpticCupVolume),
    ("OpticDiscArea", OpticDiscArea),
    ("OpticDiscKeyMeasurements", OpticDiscKeyMeasurements),
    ("Optical", Optical),
    ("OpticalCoherenceTomography", OpticalCoherenceTomography),
    ("OpticalDistanceMeter", OpticalDistanceMeter),
    ("OpticalFilterType", OpticalFilterType),
    ("OpticalFixationMeasurements", OpticalFixationMeasurements),
    ("OpticalSurfaceScanner", OpticalSurfaceScanner),
    ("OptimizedForTheDeviceInstance", OptimizedForTheDeviceInstance),
    ("OrderRecord", OrderRecord),
    ("OrganAtRisk", OrganAtRisk),
    ("OrganBasedModulation", OrganBasedModulation),
    ("OrganDose", OrganDose),
    ("OrganDoseInformation", OrganDoseInformation),
    ("OrganizationName", OrganizationName),
    ("Orientation", Orientation),
    ("OrientationDescriptor", OrientationDescriptor),
    ("OriginalSource", OriginalSource),
    ("OrthognathicFunctionalCondition", OrthognathicFunctionalCondition),
    ("OrthogonalLocationArc", OrthogonalLocationArc),
    ("OrthogonalLocationArcInnerMargin", OrthogonalLocationArcInnerMargin),
    ("OrthogonalLocationArcOuterMargin", OrthogonalLocationArcOuterMargin),
    ("OsmolalityAt37C", OsmolalityAt37C),
    ("OsmolarityAt37C", OsmolarityAt37C),
    ("Osseous", Osseous),
    ("OsseousModifier", OsseousModifier),
    ("OstiumOfLeftAuricularAppendage", OstiumOfLeftAuricularAppendage),
    ("OtherFailure", OtherFailure),
    ("OtherImageOfBiplanePair", OtherImageOfBiplanePair),
    ("OtherImageOfStereoscopicPair", OtherImageOfStereoscopicPair),
    ("OtherMalignancyType", OtherMalignancyType),
    ("OtherMarker", OtherMarker),
    ("OtherModality", OtherModality),
    ("OtherPartialViews", OtherPartialViews),
    ("OtherProjectionSystem", OtherProjectionSystem),
    ("OtherSex", OtherSex),
    ("OutOfFocus", OutOfFocus),
    ("OutOfToleranceInterlock", OutOfToleranceInterlock),
    ("OuterLimitsOfFuzzyMargin", OuterLimitsOfFuzzyMargin),
    ("OuterRetinaStructuralReflectanceMap", OuterRetinaStructuralReflectanceMap),
    ("OuterRetinaVasculatureFlow", OuterRetinaVasculatureFlow),
    ("OuterSurfaceOfBruchsMembrane", OuterSurfaceOfBruchsMembrane),
    ("OuterSurfaceOfGCL", OuterSurfaceOfGCL),
    ("OuterSurfaceOfHFL", OuterSurfaceOfHFL),
    ("OuterSurfaceOfINL", OuterSurfaceOfINL),
    ("OuterSurfaceOfIPL", OuterSurfaceOfIPL),
    ("OuterSurfaceOfOPL", OuterSurfaceOfOPL),
    ("OuterSurfaceOfRNFL", OuterSurfaceOfRNFL),
    ("OuterSurfaceOfTheCC", OuterSurfaceOfTheCC),
    ("OuterSurfaceOfTheEllipsoidZone", OuterSurfaceOfTheEllipsoidZone),
    ("OuterSurfaceOfTheInterdigitationZone", OuterSurfaceOfTheInterdigitationZone),
    ("OuterSurfaceOfTheRPE", OuterSurfaceOfTheRPE),
    ("OutflowAngle", OutflowAngle),
    ("Outline", Outline),
    ("OutlineOfLobulations", OutlineOfLobulations),
    ("OutlineOfSpiculations", OutlineOfSpiculations),
    ("OutputMeasurementPointPosition", OutputMeasurementPointPosition),
    ("OutsideNormalLimits", OutsideNormalLimits),
    ("OutsideOperationalLimitsOfTheDevice", OutsideOperationalLimitsOfTheDevice),
    ("OverExposed", OverExposed),
    ("OverallAssessment", OverallAssessment),
    ("OverallImageQualityEvaluation", OverallImageQualityEvaluation),
    ("OverallImpressionPerRecommendationAnalysis", OverallImpressionPerRecommendationAnalysis),
    ("OverallProstateFinding", OverallProstateFinding),
    ("OverallStudyQuality", OverallStudyQuality),
    ("Overflow", Overflow),
    ("OxygenAdministrationByVentilator", OxygenAdministrationByVentilator),
    ("OxygenAdministrationRate", OxygenAdministrationRate),
    ("OxygenConsumption", OxygenConsumption),
    ("OxygenExtractionFraction", OxygenExtractionFraction),
    ("PAPLeakPressure", PAPLeakPressure),
    ("PAPPressure", PAPPressure),
    ("PAPTidalVolume", PAPTidalVolume),
    ("PAS", PAS),
    ("PCIDuringThisProcedure", PCIDuringThisProcedure),
    ("PEARLDGS", PEARLDGS),
    ("PETImageReconstruction", PETImageReconstruction),
    ("PETMeasurementReport", PETMeasurementReport),
    ("PETMyocardialPerfusionRestAndStress", PETMyocardialPerfusionRestAndStress),
    ("PETMyocardialPerfusionRestOnly", PETMyocardialPerfusionRestOnly),
    ("PETMyocardialPerfusionStressOnly", PETMyocardialPerfusionStressOnly),
    ("PETMyocardialViabilityRestAndStress", PETMyocardialViabilityRestAndStress),
    ("PETMyocardialViabilityRestOnly", PETMyocardialViabilityRestOnly),
    ("PETMyocardialViabilityStressOnly", PETMyocardialViabilityStressOnly),
    ("PETRadionuclideIncubationTime", PETRadionuclideIncubationTime),
    ("PGreaterThan5Percent", PGreaterThan5Percent),
    ("PIRADS20ProstateMRIAcquisitionRequirements", PIRADS20ProstateMRIAcquisitionRequirements),
    ("PIRADS21ProstateMRIAcquisitionRequirements", PIRADS21ProstateMRIAcquisitionRequirements),
    ("PIRADSDCELesionAssessment", PIRADSDCELesionAssessment),
    ("PIRADSDWILesionAssessment", PIRADSDWILesionAssessment),
    ("PIRADSLocalizedAbnormalityAssessment", PIRADSLocalizedAbnormalityAssessment),
    ("PIRADST2WILesionAssessment", PIRADST2WILesionAssessment),
    ("PIRADSV20", PIRADSV20),
    ("PIRADSV21", PIRADSV21),
    ("PISARadius", PISARadius),
    ("PLessThan05Percent", PLessThan05Percent),
    ("PLessThan1Percent", PLessThan1Percent),
    ("PLessThan2Percent", PLessThan2Percent),
    ("PLessThan5Percent", PLessThan5Percent),
    ("PO10", PO10),
    ("PO9", PO9),
    ("PSMA1007F18", PSMA1007F18),
    ("PSMA617Ga68", PSMA617Ga68),
    ("PTVNodal", PTVNodal),
    ("PTVPrimary", PTVPrimary),
    ("PVVTIForward", PVVTIForward),
    ("PVVTIRPerVTIFRatio", PVVTIRPerVTIFRatio),
    ("PVVTIReverse", PVVTIReverse),
    ("PWave", PWave),
    ("Paced", Paced),
    ("PacedStressTest", PacedStressTest),
    ("Pacemaker", Pacemaker),
    ("PacingElectricalStimulusVoltage", PacingElectricalStimulusVoltage),
    ("PackageInsert", PackageInsert),
    ("Paddle", Paddle),
    ("PageSentTo", PageSentTo),
    ("PalpationGuided", PalpationGuided),
    ("PancreaticStent", PancreaticStent),
    ("Panitumumab89Zr", Panitumumab89Zr),
    ("PanoramicXRay", PanoramicXRay),
    ("PaperBasedBedding", PaperBasedBedding),
    ("PaperDigitized", PaperDigitized),
    ("PaperDocument", PaperDocument),
    ("PapillaryCarcinomaInSitu", PapillaryCarcinomaInSitu),
    ("PapillaryMuscleExcluded", PapillaryMuscleExcluded),
    ("PapillaryMuscleIncluded", PapillaryMuscleIncluded),
    ("PapillaryMuscleIncludedPerExcluded", PapillaryMuscleIncludedPerExcluded),
    ("Parallel", Parallel),
    ("ParallelGrid", ParallelGrid),
    ("ParallelImaging", ParallelImaging),
    ("Parallelepiped", Parallelepiped),
    ("ParaspinalLine", ParaspinalLine),
    ("ParenchymalBand", ParenchymalBand),
    ("ParentSpecimenIdentifier", ParentSpecimenIdentifier),
    ("ParentSpecimenType", ParentSpecimenType),
    ("ParentVesselFinding", ParentVesselFinding),
    ("PartialFlatteningFilterBeam", PartialFlatteningFilterBeam),
    ("PartialRenderingForPresentation", PartialRenderingForPresentation),
    ("PartialReport", PartialReport),
    ("PartiallyEncapsulated", PartiallyEncapsulated),
    ("PartiallySucceeded", PartiallySucceeded),
    ("ParticleDisplacementMethod", ParticleDisplacementMethod),
    ("ParticleVelocityMethod", ParticleVelocityMethod),
    ("Pasquini2007", Pasquini2007),
    ("PatencyTestInjection", PatencyTestInjection),
    ("Path", Path),
    ("PathLength", PathLength),
    ("PathVertex", PathVertex),
    ("Path_121210", Path_121210),
    ("Pathology", Pathology),
    ("PathologyResults", PathologyResults),
    ("Patient", Patient),
    ("PatientAPDimension", PatientAPDimension),
    ("PatientAdmittedToProcedureRoom", PatientAdmittedToProcedureRoom),
    ("PatientAdvanceDirectiveGiven", PatientAdvanceDirectiveGiven),
    ("PatientAlert", PatientAlert),
    ("PatientAlignmentProcedure", PatientAlignmentProcedure),
    ("PatientAllergicToMediaPerContrast", PatientAllergicToMediaPerContrast),
    ("PatientAnatomyModel", PatientAnatomyModel),
    ("PatientAsleep", PatientAsleep),
    ("PatientAssessmentPerformed", PatientAssessmentPerformed),
    ("PatientAssistedToTable", PatientAssistedToTable),
    ("PatientAttachedDoseControlObject", PatientAttachedDoseControlObject),
    ("PatientAttenuationCharacteristics", PatientAttenuationCharacteristics),
    ("PatientCalledToProcedureRoom", PatientCalledToProcedureRoom),
    ("PatientCharacteristics", PatientCharacteristics),
    ("PatientClearanceInterlock", PatientClearanceInterlock),
    ("PatientConditionPreventedContinuing", PatientConditionPreventedContinuing),
    ("PatientConnectedToContinuousMonitoring", PatientConnectedToContinuousMonitoring),
    ("PatientConsciousness", PatientConsciousness),
    ("PatientCoordinateSystemOrigin", PatientCoordinateSystemOrigin),
    ("PatientCoughed", PatientCoughed),
    ("PatientDataUsedDuringPlanning", PatientDataUsedDuringPlanning),
    ("PatientDecisionToTerminateTreatment", PatientDecisionToTerminateTreatment),
    ("PatientDidNotArrive", PatientDidNotArrive),
    ("PatientDied", PatientDied),
    ("PatientDischargedFromDepartment", PatientDischargedFromDepartment),
    ("PatientDisconnectedFromContinuousMonitoring", PatientDisconnectedFromContinuousMonitoring),
    ("PatientDisoriented", PatientDisoriented),
    ("PatientDistractionDevice", PatientDistractionDevice),
    ("PatientElevation", PatientElevation),
    ("PatientEquivalentThickness", PatientEquivalentThickness),
    ("PatientExposureToIonizingRadiation", PatientExposureToIonizingRadiation),
    ("PatientFixationProcedure", PatientFixationProcedure),
    ("PatientGivenPostProcedureInstruction", PatientGivenPostProcedureInstruction),
    ("PatientGivenPreProcedureInstruction", PatientGivenPreProcedureInstruction),
    ("PatientHasRespiratoryDifficulty", PatientHasRespiratoryDifficulty),
    ("PatientImage", PatientImage),
    ("PatientInformedConsentGiven", PatientInformedConsentGiven),
    ("PatientLateralDimension", PatientLateralDimension),
    ("PatientLocationFiducial", PatientLocationFiducial),
    ("PatientModel", PatientModel),
    ("PatientModelDemographics", PatientModelDemographics),
    ("PatientModelRegistration", PatientModelRegistration),
    ("PatientModelType", PatientModelType),
    ("PatientMotion", PatientMotion),
    ("PatientMotionInterlock", PatientMotionInterlock),
    ("PatientMotionManagementSetupProcedure", PatientMotionManagementSetupProcedure),
    ("PatientMovement", PatientMovement),
    ("PatientOrientation", PatientOrientation),
    ("PatientOrientationColumn", PatientOrientationColumn),
    ("PatientOrientationModifier", PatientOrientationModifier),
    ("PatientOrientationRow", PatientOrientationRow),
    ("PatientPosition", PatientPosition),
    ("PatientPositioningNote", PatientPositioningNote),
    ("PatientPositioningProcedureNote", PatientPositioningProcedureNote),
    ("PatientPregnant", PatientPregnant),
    ("PatientPreppedAndDraped", PatientPreppedAndDraped),
    ("PatientPresentation", PatientPresentation),
    ("PatientPronouncedDead", PatientPronouncedDead),
    ("PatientRadiationDoseModel", PatientRadiationDoseModel),
    ("PatientRadiationDoseModelData", PatientRadiationDoseModelData),
    ("PatientRadiationDoseModelReference", PatientRadiationDoseModelReference),
    ("PatientRadiationDoseReport", PatientRadiationDoseReport),
    ("PatientRecord", PatientRecord),
    ("PatientRefusedToContinueProcedure", PatientRefusedToContinueProcedure),
    ("PatientReportsChestPain", PatientReportsChestPain),
    ("PatientReportsDiscomfort", PatientReportsDiscomfort),
    ("PatientReportsNausea", PatientReportsNausea),
    ("PatientReportsNoPain", PatientReportsNoPain),
    ("PatientRepositioned", PatientRepositioned),
    ("PatientRestless", PatientRestless),
    ("PatientRotationLongitudinal", PatientRotationLongitudinal),
    ("PatientSedated", PatientSedated),
    ("PatientSegmentedModel", PatientSegmentedModel),
    ("PatientSetupNote", PatientSetupNote),
    ("PatientSetupPoint", PatientSetupPoint),
    ("PatientSetupVerificationInputUsed", PatientSetupVerificationInputUsed),
    ("PatientSetupVerificationResult", PatientSetupVerificationResult),
    ("PatientShieldingProcedure", PatientShieldingProcedure),
    ("PatientState", PatientState),
    ("PatientStatusOrEvent", PatientStatusOrEvent),
    ("PatientSupport", PatientSupport),
    ("PatientSupportOrigin", PatientSupportOrigin),
    ("PatientTableRelationship", PatientTableRelationship),
    ("PatientTakenForTreatmentOrSurgery", PatientTakenForTreatmentOrSurgery),
    ("PatientTransferredFrom", PatientTransferredFrom),
    ("PatientTransferredToCCU", PatientTransferredToCCU),
    ("PatientTransferredToHoldingArea", PatientTransferredToHoldingArea),
    ("PatientTransferredToMorgue", PatientTransferredToMorgue),
    ("PatientTransferredToSurgery", PatientTransferredToSurgery),
    ("PatientUnresponsive", PatientUnresponsive),
    ("PatientWeightExceedsEquipmentLimit", PatientWeightExceedsEquipmentLimit),
    ("PatternEvent", PatternEvent),
    ("PatternProjection", PatternProjection),
    ("PeakActivityWorkload", PeakActivityWorkload),
    ("PeakBloodPressure", PeakBloodPressure),
    ("PeakDoubleProduct", PeakDoubleProduct),
    ("PeakEjectionRate", PeakEjectionRate),
    ("PeakEjectionTime", PeakEjectionTime),
    ("PeakFillingRate", PeakFillingRate),
    ("PeakFillingTime", PeakFillingTime),
    ("PeakFlowRateInPhaseActivity", PeakFlowRateInPhaseActivity),
    ("PeakLongitudinalLagrangianStrain", PeakLongitudinalLagrangianStrain),
    ("PeakOfThermalCardiacOutputBolus", PeakOfThermalCardiacOutputBolus),
    ("PeakPressureInPhaseActivity", PeakPressureInPhaseActivity),
    ("PeakStressState", PeakStressState),
    ("PeakTissueVelocity", PeakTissueVelocity),
    ("PeakValueWithinROI", PeakValueWithinROI),
    ("PeakVelocityIndex", PeakVelocityIndex),
    ("PectoralMuscleOutline", PectoralMuscleOutline),
    ("PediatricCardiacUltrasoundReport", PediatricCardiacUltrasoundReport),
    ("PediatricImagingSpecialty", PediatricImagingSpecialty),
    ("PediatricImagingSubjectMatter", PediatricImagingSubjectMatter),
    ("PediatricRadiology", PediatricRadiology),
    ("Pegdinetanib89Zr", Pegdinetanib89Zr),
    ("PelvisAndUterus", PelvisAndUterus),
    ("PencilBeamConvolution", PencilBeamConvolution),
    ("PercentFibroglandularTissue", PercentFibroglandularTissue),
    ("PercentageOfExpectedBeamOnTimeOfRadiation", PercentageOfExpectedBeamOnTimeOfRadiation),
    ("PercentageOfVascularCalcification", PercentageOfVascularCalcification),
    ("PercentileRankingOfMeasurement", PercentileRankingOfMeasurement),
    ("PercutaneousCoronaryIntervention", PercutaneousCoronaryIntervention),
    ("PercutaneousEntryAction", PercutaneousEntryAction),
    ("PercutaneousSiliconeInjection", PercutaneousSiliconeInjection),
    ("Perfluorocarbon", Perfluorocarbon),
    ("PerformanceOfCTForDetectionOfPulmonaryEmbolismInAdults", PerformanceOfCTForDetectionOfPulmonaryEmbolismInAdults),
    ("PerformanceOfHighResolutionCTOfTheLungsInAdults", PerformanceOfHighResolutionCTOfTheLungsInAdults),
    ("PerformanceOfPediatricAndAdultChestRadiographyACR", PerformanceOfPediatricAndAdultChestRadiographyACR),
    ("PerformanceOfPediatricAndAdultThoracicCT", PerformanceOfPediatricAndAdultThoracicCT),
    ("PerformedImagingAgentAdministration", PerformedImagingAgentAdministration),
    ("PerformedProcedureStep", PerformedProcedureStep),
    ("PerformedProcedureStepSOPClassUID", PerformedProcedureStepSOPClassUID),
    ("PerformedProcedureStepSOPInstanceUID", PerformedProcedureStepSOPInstanceUID),
    ("Performing", Performing),
    ("PerformingPhysician", PerformingPhysician),
    ("Perfusion", Perfusion),
    ("PerfusionAnalysisByArterialSpinLabelingMRTechnique", PerfusionAnalysisByArterialSpinLabelingMRTechnique),
    ("PerfusionAnalysisByIVIodinatedContrastCTTechnique", PerfusionAnalysisByIVIodinatedContrastCTTechnique),
    ("PerfusionAnalysisByStableXenonCTTechnique", PerfusionAnalysisByStableXenonCTTechnique),
    ("PerfusionAnalysisBySusceptibilityMRTechnique", PerfusionAnalysisBySusceptibilityMRTechnique),
    ("PerfusionImageAnalysis", PerfusionImageAnalysis),
    ("PeriOperativePhotographicImaging", PeriOperativePhotographicImaging),
    ("PeriProceduralMIOccurred", PeriProceduralMIOccurred),
    ("PerilesionalWhiteMatter", PerilesionalWhiteMatter),
    ("PerimeterOutline", PerimeterOutline),
    ("PerimeterOutline_121213", PerimeterOutline_121213),
    ("PeripheralDuctPapillomas", PeripheralDuctPapillomas),
    ("PeripheralInferior", PeripheralInferior),
    ("PeripheralInferiorNasal", PeripheralInferiorNasal),
    ("PeripheralInferiorTemporal", PeripheralInferiorTemporal),
    ("PeripheralNasal", PeripheralNasal),
    ("PeripheralSuperior", PeripheralSuperior),
    ("PeripheralSuperiorNasal", PeripheralSuperiorNasal),
    ("PeripheralSuperiorTemporal", PeripheralSuperiorTemporal),
    ("PeripheralTemporal", PeripheralTemporal),
    ("PerivascularAdiposeTissueFatAttenuationIndex", PerivascularAdiposeTissueFatAttenuationIndex),
    ("PersistentDelayedPhaseType1Curve", PersistentDelayedPhaseType1Curve),
    ("Person", Person),
    ("PersonAdministeringDrugPerContrast", PersonAdministeringDrugPerContrast),
    ("PersonID", PersonID),
    ("PersonIDIssuer", PersonIDIssuer),
    ("PersonName", PersonName),
    ("PersonObserverName", PersonObserverName),
    ("PersonObserversLoginName", PersonObserversLoginName),
    ("PersonObserversOrganizationName", PersonObserversOrganizationName),
    ("PersonObserversRoleInTheOrganization", PersonObserversRoleInTheOrganization),
    ("PersonObserversRoleInThisProcedure", PersonObserversRoleInThisProcedure),
    ("PersonRoleInOrganization", PersonRoleInOrganization),
    ("PersonRoleInProcedure", PersonRoleInProcedure),
    ("PersonalBreastCancerHistory", PersonalBreastCancerHistory),
    ("PersonalContent", PersonalContent),
    ("PersonalHistoryOfBreastCancerWithBreastConservationTherapy", PersonalHistoryOfBreastCancerWithBreastConservationTherapy),
    ("PersonalHistoryOfBreastCancerWithMastectomy", PersonalHistoryOfBreastCancerWithMastectomy),
    ("PersonnelArrived", PersonnelArrived),
    ("PersonnelDeparted", PersonnelDeparted),
    ("Phantom", Phantom),
    ("PhantomTumorPseudotumor", PhantomTumorPseudotumor),
    ("PharmacologicAndExerciseStressTest", PharmacologicAndExerciseStressTest),
    ("PharmacologicalStressAgentDose", PharmacologicalStressAgentDose),
    ("PharmacologicalStressAgentDoseRate", PharmacologicalStressAgentDoseRate),
    ("PhaseContrastIllumination", PhaseContrastIllumination),
    ("PhaseOfAnimalHandling", PhaseOfAnimalHandling),
    ("PhasedBeamSteering", PhasedBeamSteering),
    ("Phonocardiogram", Phonocardiogram),
    ("Photoacoustic", Photoacoustic),
    ("PhotoelectricEffect", PhotoelectricEffect),
    ("PhotogrammetricImaging3DCAMModel", PhotogrammetricImaging3DCAMModel),
    ("PhotonEnergy", PhotonEnergy),
    ("PhotonFixedAperture", PhotonFixedAperture),
    ("PhotopicLight", PhotopicLight),
    ("PhysicalCompensator", PhysicalCompensator),
    ("PhysicalExaminationResults", PhysicalExaminationResults),
    ("PhysicalForce", PhysicalForce),
    ("Physician", Physician),
    ("PhysicianDecisionToTerminateTreatment", PhysicianDecisionToTerminateTreatment),
    ("PhysicianNote", PhysicianNote),
    ("PhysicianReviewInputUsed", PhysicianReviewInputUsed),
    ("PhysicistReviewInputUsed", PhysicistReviewInputUsed),
    ("PhysicsAssistant", PhysicsAssistant),
    ("PhysiologicalAudioSignal", PhysiologicalAudioSignal),
    ("PhysiologicalChallenges", PhysiologicalChallenges),
    ("PiezocompositeTransducer", PiezocompositeTransducer),
    ("PinatuzumabVedotin89Zr", PinatuzumabVedotin89Zr),
    ("PineChipBedding", PineChipBedding),
    ("PineShavingBedding", PineShavingBedding),
    ("Pinning", Pinning),
    ("PitchFactor", PitchFactor),
    ("PittsburghCompoundBC11", PittsburghCompoundBC11),
    ("PixelByPixelAddition", PixelByPixelAddition),
    ("PixelByPixelDivision", PixelByPixelDivision),
    ("PixelByPixelMask", PixelByPixelMask),
    ("PixelByPixelMaximum", PixelByPixelMaximum),
    ("PixelByPixelMean", PixelByPixelMean),
    ("PixelByPixelMinimum", PixelByPixelMinimum),
    ("PixelByPixelMultiplication", PixelByPixelMultiplication),
    ("PixelByPixelSubtraction", PixelByPixelSubtraction),
    ("PixelDataColumns", PixelDataColumns),
    ("PixelDataRows", PixelDataRows),
    ("PixelFaultsEvaluation", PixelFaultsEvaluation),
    ("PixelatedSpiculations", PixelatedSpiculations),
    ("PlacerNumber", PlacerNumber),
    ("Plan", Plan),
    ("PlanComparisonInputUsed", PlanComparisonInputUsed),
    ("PlanMeetsPrescription", PlanMeetsPrescription),
    ("PlanQAPassed", PlanQAPassed),
    ("PlanQualityAssuranceInputUsed", PlanQualityAssuranceInputUsed),
    ("PlanarMatrixUltrasoundTransducerGeometry", PlanarMatrixUltrasoundTransducerGeometry),
    ("PlaneA", PlaneA),
    ("PlaneB", PlaneB),
    ("PlaneScanPattern", PlaneScanPattern),
    ("PlaneThroughAnteriorExtent", PlaneThroughAnteriorExtent),
    ("PlaneThroughCenter", PlaneThroughCenter),
    ("PlaneThroughDistalExtent", PlaneThroughDistalExtent),
    ("PlaneThroughInferiorExtent", PlaneThroughInferiorExtent),
    ("PlaneThroughLateralExtent", PlaneThroughLateralExtent),
    ("PlaneThroughLeftmostExtent", PlaneThroughLeftmostExtent),
    ("PlaneThroughMedialExtent", PlaneThroughMedialExtent),
    ("PlaneThroughPosteriorExtent", PlaneThroughPosteriorExtent),
    ("PlaneThroughProximalExtent", PlaneThroughProximalExtent),
    ("PlaneThroughRightmostExtent", PlaneThroughRightmostExtent),
    ("PlaneThroughSuperiorExtent", PlaneThroughSuperiorExtent),
    ("Planimetry", Planimetry),
    ("PlannedImagingAgentAdministration", PlannedImagingAgentAdministration),
    ("PlannedImagingAgentAdministrationProcedureReport", PlannedImagingAgentAdministrationProcedureReport),
    ("PlannedImagingAgentAdministrationSOPInstance", PlannedImagingAgentAdministrationSOPInstance),
    ("PlanningImagingNote", PlanningImagingNote),
    ("PlanningInformationForIntraoperativeUsage", PlanningInformationForIntraoperativeUsage),
    ("PlanningIntent", PlanningIntent),
    ("PlanningMethod", PlanningMethod),
    ("PlanningOrganAtRiskVolume", PlanningOrganAtRiskVolume),
    ("PlaqueArea", PlaqueArea),
    ("PlaqueBurden", PlaqueBurden),
    ("PlaquePlusMediaCrossSectionalArea", PlaquePlusMediaCrossSectionalArea),
    ("PlaquePlusMediaEccentricityIndex", PlaquePlusMediaEccentricityIndex),
    ("PlaquePlusMediaThickness", PlaquePlusMediaThickness),
    ("PlaqueRupture", PlaqueRupture),
    ("PlasmaDisplay", PlasmaDisplay),
    ("PlateauDelayedPhaseType2Curve", PlateauDelayedPhaseType2Curve),
    ("Pleonemia", Pleonemia),
    ("PointCloudAlgorithmic", PointCloudAlgorithmic),
    ("PointShearWaveElastography", PointShearWaveElastography),
    ("PoiseuilleResistance", PoiseuilleResistance),
    ("PolarToRectangularScanConversion", PolarToRectangularScanConversion),
    ("PolarizingFilter", PolarizingFilter),
    ("PolatuzumabVedotin89Zr", PolatuzumabVedotin89Zr),
    ("PolypStalkWidth", PolypStalkWidth),
    ("PolyurethaneImplant", PolyurethaneImplant),
    ("PoorContrastTiming", PoorContrastTiming),
    ("PoorFOVSelection", PoorFOVSelection),
    ("PoorImageQuality", PoorImageQuality),
    ("PoorSNR", PoorSNR),
    ("PoorlyDemarcated", PoorlyDemarcated),
    ("PopulationAveragedAIF", PopulationAveragedAIF),
    ("PopulationDescription", PopulationDescription),
    ("PopulationIndex", PopulationIndex),
    ("PortableMediaImporterEquipment", PortableMediaImporterEquipment),
    ("Portacath", Portacath),
    ("PositionFrameOfReference", PositionFrameOfReference),
    ("PositionOfDistalBorder", PositionOfDistalBorder),
    ("PositionOfProximalBorder", PositionOfProximalBorder),
    ("PositionReferenceIndicator", PositionReferenceIndicator),
    ("PositionSensor", PositionSensor),
    ("PositionerIsocenterDetectorRotationAngle", PositionerIsocenterDetectorRotationAngle),
    ("PositionerIsocenterDetectorRotationEndAngle", PositionerIsocenterDetectorRotationEndAngle),
    ("PositionerIsocenterPrimaryAngle", PositionerIsocenterPrimaryAngle),
    ("PositionerIsocenterPrimaryEndAngle", PositionerIsocenterPrimaryEndAngle),
    ("PositionerIsocenterSecondaryAngle", PositionerIsocenterSecondaryAngle),
    ("PositionerIsocenterSecondaryEndAngle", PositionerIsocenterSecondaryEndAngle),
    ("PositionerPrimaryAngle", PositionerPrimaryAngle),
    ("PositionerPrimaryEndAngle", PositionerPrimaryEndAngle),
    ("PositionerSecondaryAngle", PositionerSecondaryAngle),
    ("PositionerSecondaryEndAngle", PositionerSecondaryEndAngle),
    ("Positioning", Positioning),
    ("PositiveDCE", PositiveDCE),
    ("PositiveInfinity", PositiveInfinity),
    ("PositronEmissionTomography", PositronEmissionTomography),
    ("PostAdministrationMeasuredActivity", PostAdministrationMeasuredActivity),
    ("PostCoordinatedMeasurements", PostCoordinatedMeasurements),
    ("PostHyperventilation", PostHyperventilation),
    ("PostIntervention", PostIntervention),
    ("PostInterventionStenosisMeasurement", PostInterventionStenosisMeasurement),
    ("PostInterventionTIMIFlow", PostInterventionTIMIFlow),
    ("PostLeftAtriumSpaceIndex", PostLeftAtriumSpaceIndex),
    ("PostMenopausalPatient", PostMenopausalPatient),
    ("PostProceduralStructuralHeartMeasurementReport", PostProceduralStructuralHeartMeasurementReport),
    ("PostProcedureMammogramsForMarkerPlacement", PostProcedureMammogramsForMarkerPlacement),
    ("PostReductionMammoplasty", PostReductionMammoplasty),
    ("PostVoiding", PostVoiding),
    ("PosteriorAcousticFeatures", PosteriorAcousticFeatures),
    ("PosteriorCorneaSurfaceMeasurementMethod", PosteriorCorneaSurfaceMeasurementMethod),
    ("PosteriorEnhancement", PosteriorEnhancement),
    ("PosteriorFirst", PosteriorFirst),
    ("PosteriorJunctionLine", PosteriorJunctionLine),
    ("PosteriorLens", PosteriorLens),
    ("PosteriorMitralValveLeafletArea", PosteriorMitralValveLeafletArea),
    ("PosteriorMitralValveLeafletLength", PosteriorMitralValveLeafletLength),
    ("PosteriorMitralValveP1LeafletScallopLength", PosteriorMitralValveP1LeafletScallopLength),
    ("PosteriorMitralValveP2LeafletScallopLength", PosteriorMitralValveP2LeafletScallopLength),
    ("PosteriorMitralValveP3LeafletScallopLength", PosteriorMitralValveP3LeafletScallopLength),
    ("PosteriorPole8x8Grid", PosteriorPole8x8Grid),
    ("PosteriorShadowing", PosteriorShadowing),
    ("PosteriorToAnterior", PosteriorToAnterior),
    ("PosteriorTrachealStripe", PosteriorTrachealStripe),
    ("PosteromedialPapillaryMuscleToTheRightTrigone", PosteromedialPapillaryMuscleToTheRightTrigone),
    ("Posttreatment", Posttreatment),
    ("PowerDoppler", PowerDoppler),
    ("PreAdministrationMeasuredActivity", PreAdministrationMeasuredActivity),
    ("PreBiopsyLocalizationOfProstateLesion", PreBiopsyLocalizationOfProstateLesion),
    ("PreCoordinatedMeasurements", PreCoordinatedMeasurements),
    ("PreEjectionPeriod", PreEjectionPeriod),
    ("PreHeatedPad", PreHeatedPad),
    ("PreIntervention", PreIntervention),
    ("PreMedication", PreMedication),
    ("PrePectoralImplant", PrePectoralImplant),
    ("PrePlanningResult", PrePlanningResult),
    ("PreProceduralStructuralHeartMeasurementReport", PreProceduralStructuralHeartMeasurementReport),
    ("PreProcedureLog", PreProcedureLog),
    ("PreSurgicalAnatomy", PreSurgicalAnatomy),
    ("PreamplifierConnected", PreamplifierConnected),
    ("PreamplifierDisconnected", PreamplifierDisconnected),
    ("PreclinicalSmallAnimalImagingAcquisitionContext", PreclinicalSmallAnimalImagingAcquisitionContext),
    ("PredecessorContainingGroupOfImagingSubjects", PredecessorContainingGroupOfImagingSubjects),
    ("PregnancyStatus", PregnancyStatus),
    ("PreliminaryReport", PreliminaryReport),
    ("PreparationForImaging", PreparationForImaging),
    ("PreparationRoom", PreparationRoom),
    ("PrescriptionIdentifier", PrescriptionIdentifier),
    ("PrescriptionRadiationDose", PrescriptionRadiationDose),
    ("PrescriptionVisualAcuity", PrescriptionVisualAcuity),
    ("PresenceOfProperty", PresenceOfProperty),
    ("PresenceUndetermined", PresenceUndetermined),
    ("Present", Present),
    ("PresentationOptionalRenderingDeviceMayPresent", PresentationOptionalRenderingDeviceMayPresent),
    ("PresentationRequiredRenderingDeviceIsExpectedToPresent", PresentationRequiredRenderingDeviceIsExpectedToPresent),
    ("PresentationState", PresentationState),
    ("PressureAboveAdjustmentLimit", PressureAboveAdjustmentLimit),
    ("PressureAboveWarningLimit", PressureAboveWarningLimit),
    ("PressureAtDpPerDtMax", PressureAtDpPerDtMax),
    ("PressureDropAtSFR", PressureDropAtSFR),
    ("PressureLimit", PressureLimit),
    ("PressureVsTime", PressureVsTime),
    ("PreviousFetalMalformationPerSyndrome", PreviousFetalMalformationPerSyndrome),
    ("PreviousFinding", PreviousFinding),
    ("PreviousFindings", PreviousFindings),
    ("PreviousLBWOrIUGRBirth", PreviousLBWOrIUGRBirth),
    ("PreviousProcedure", PreviousProcedure),
    ("PreviousRHNegativeOrBloodDyscrasiaAtBirth", PreviousRHNegativeOrBloodDyscrasiaAtBirth),
    ("PreviousReports", PreviousReports),
    ("PreviousTreatmentNote", PreviousTreatmentNote),
    ("PrimaryComplex", PrimaryComplex),
    ("PrimaryFluenceMonitoringSystemInterlock", PrimaryFluenceMonitoringSystemInterlock),
    ("PrimaryFollicle", PrimaryFollicle),
    ("PrimaryInterventionDevice", PrimaryInterventionDevice),
    ("PrimaryLevel", PrimaryLevel),
    ("PrimordialFollicle", PrimordialFollicle),
    ("Print", Print),
    ("PriorProcedureDescriptions", PriorProcedureDescriptions),
    ("PriorReportForCurrentPatient", PriorReportForCurrentPatient),
    ("PriorToVoiding", PriorToVoiding),
    ("Probabilistic", Probabilistic),
    ("Probability", Probability),
    ("ProbabilityOfCancer", ProbabilityOfCancer),
    ("ProcedureActionDuration", ProcedureActionDuration),
    ("ProcedureActionID", ProcedureActionID),
    ("ProcedureAnnotated", ProcedureAnnotated),
    ("ProcedureCharacteristics", ProcedureCharacteristics),
    ("ProcedureCode", ProcedureCode),
    ("ProcedureDateTime", ProcedureDateTime),
    ("ProcedureDescription", ProcedureDescription),
    ("ProcedureEnvironmentalCharacteristics", ProcedureEnvironmentalCharacteristics),
    ("ProcedureLogging", ProcedureLogging),
    ("ProcedureModifier", ProcedureModifier),
    ("ProcedureNote", ProcedureNote),
    ("ProcedureRecord", ProcedureRecord),
    ("ProcedureReported", ProcedureReported),
    ("ProcedureResult", ProcedureResult),
    ("ProcedureStepNumber", ProcedureStepNumber),
    ("ProcedureStepToThisPoint", ProcedureStepToThisPoint),
    ("ProcedureStudyComponentUID", ProcedureStudyComponentUID),
    ("ProcedureStudyInstanceUID", ProcedureStudyInstanceUID),
    ("ProcedureTimeBase", ProcedureTimeBase),
    ("ProcessedKeyObjectSelection", ProcessedKeyObjectSelection),
    ("ProcessingEquipment", ProcessingEquipment),
    ("ProcessingStepDescription", ProcessingStepDescription),
    ("ProcessingType", ProcessingType),
    ("Profusion", Profusion),
    ("ProgesteroneReceptor", ProgesteroneReceptor),
    ("ProgrammableInjectorDevice", ProgrammableInjectorDevice),
    ("ProgrammedHoldStarted", ProgrammedHoldStarted),
    ("ProjectionEponymousName", ProjectionEponymousName),
    ("ProjectionSpaceMAR", ProjectionSpaceMAR),
    ("ProjectionXRay", ProjectionXRay),
    ("ProliferativeActivity", ProliferativeActivity),
    ("ProspectiveGating", ProspectiveGating),
    ("ProstateImagingFindings", ProstateImagingFindings),
    ("ProstateMRIRelevantProcedureInformation", ProstateMRIRelevantProcedureInformation),
    ("ProstateRelationalMeasurements", ProstateRelationalMeasurements),
    ("ProstheticFabrication", ProstheticFabrication),
    ("ProstheticShaftAugment", ProstheticShaftAugment),
    ("ProtocolNotFollowed", ProtocolNotFollowed),
    ("ProtocolStage", ProtocolStage),
    ("ProtocolTimePointIdentifier", ProtocolTimePointIdentifier),
    ("ProtonDensity", ProtonDensity),
    ("ProtonDensityWeightedMRSignalIntensity", ProtonDensityWeightedMRSignalIntensity),
    ("ProximalFindingSite", ProximalFindingSite),
    ("ProximalIsovelocitySurfaceArea", ProximalIsovelocitySurfaceArea),
    ("ProximalReference", ProximalReference),
    ("ProximalStentMargin", ProximalStentMargin),
    ("PseudoangiomatousStromalHyperplasia", PseudoangiomatousStromalHyperplasia),
    ("Pseudoplaque", Pseudoplaque),
    ("PulmonaryArterialContentFCpa", PulmonaryArterialContentFCpa),
    ("PulmonaryFlow", PulmonaryFlow),
    ("PulmonaryImagingSubjectMatter", PulmonaryImagingSubjectMatter),
    ("PulmonaryVenousContentFCpv", PulmonaryVenousContentFCpv),
    ("PulseRate", PulseRate),
    ("PulseSequenceName", PulseSequenceName),
    ("PulseStrength", PulseStrength),
    ("PulseTrainDefinition", PulseTrainDefinition),
    ("PulseWidth", PulseWidth),
    ("Pulsed", Pulsed),
    ("QANode", QANode),
    ("QANodePair", QANodePair),
    ("QAProcessNote", QAProcessNote),
    ("QWave", QWave),
    ("QuadrantLocation", QuadrantLocation),
    ("QuadrantOctantSectors", QuadrantOctantSectors),
    ("QuadrantSectors", QuadrantSectors),
    ("QuadraticCellKillFactor", QuadraticCellKillFactor),
    ("QualitativeAnalysis", QualitativeAnalysis),
    ("QualitativeDifference", QualitativeDifference),
    ("QualityAssessment", QualityAssessment),
    ("QualityControl", QualityControl),
    ("QualityControlIntent", QualityControlIntent),
    ("QualityControlMaterialImage", QualityControlMaterialImage),
    ("QualityControlStandard", QualityControlStandard),
    ("QualityFinding", QualityFinding),
    ("QualityIssue", QualityIssue),
    ("QuantitativeAnalysis", QuantitativeAnalysis),
    ("QuantitativeArteriographyReport", QuantitativeArteriographyReport),
    ("QuantitativeVentriculographyReport", QuantitativeVentriculographyReport),
    ("QuantityAdministered", QuantityAdministered),
    ("QuantityOfMaterial", QuantityOfMaterial),
    ("Query", Query),
    ("QuotationMode", QuotationMode),
    ("QuotedSource", QuotedSource),
    ("R1", R1),
    ("R150789Zr", R150789Zr),
    ("R2Coefficient", R2Coefficient),
    ("R2_126394", R2_126394),
    ("R2_126395", R2_126395),
    ("RADAR2017", RADAR2017),
    ("RCARegionInLAOProjection", RCARegionInLAOProjection),
    ("RCARegionInRAOProjection", RCARegionInRAOProjection),
    ("RCoefficient", RCoefficient),
    ("RDSRFrameOfReferenceDescription", RDSRFrameOfReferenceDescription),
    ("RDSRFrameOfReferenceOrigin", RDSRFrameOfReferenceOrigin),
    ("RECIST", RECIST),
    ("RECIST10", RECIST10),
    ("RECIST11", RECIST11),
    ("RGBBComponent", RGBBComponent),
    ("RGBGComponent", RGBGComponent),
    ("RGBRComponent", RGBRComponent),
    ("RNFLAverageThickness", RNFLAverageThickness),
    ("RNFLClockfaceMethod", RNFLClockfaceMethod),
    ("RNFLClockfacePosition10Thickness", RNFLClockfacePosition10Thickness),
    ("RNFLClockfacePosition11Thickness", RNFLClockfacePosition11Thickness),
    ("RNFLClockfacePosition12Thickness", RNFLClockfacePosition12Thickness),
    ("RNFLClockfacePosition1Thickness", RNFLClockfacePosition1Thickness),
    ("RNFLClockfacePosition2Thickness", RNFLClockfacePosition2Thickness),
    ("RNFLClockfacePosition3Thickness", RNFLClockfacePosition3Thickness),
    ("RNFLClockfacePosition4Thickness", RNFLClockfacePosition4Thickness),
    ("RNFLClockfacePosition5Thickness", RNFLClockfacePosition5Thickness),
    ("RNFLClockfacePosition6Thickness", RNFLClockfacePosition6Thickness),
    ("RNFLClockfacePosition7Thickness", RNFLClockfacePosition7Thickness),
    ("RNFLClockfacePosition8Thickness", RNFLClockfacePosition8Thickness),
    ("RNFLClockfacePosition9Thickness", RNFLClockfacePosition9Thickness),
    ("RNFLInferiorSectorThickness", RNFLInferiorSectorThickness),
    ("RNFLNasalInferiorSectorThickness", RNFLNasalInferiorSectorThickness),
    ("RNFLNasalSectorThickness", RNFLNasalSectorThickness),
    ("RNFLNasalSuperiorSectorThickness", RNFLNasalSuperiorSectorThickness),
    ("RNFLSuperiorSectorThickness", RNFLSuperiorSectorThickness),
    ("RNFLTemporalInferiorSectorThickness", RNFLTemporalInferiorSectorThickness),
    ("RNFLTemporalSectorThickness", RNFLTemporalSectorThickness),
    ("RNFLTemporalSuperiorSectorThickness", RNFLTemporalSuperiorSectorThickness),
    ("RNFLVascularPlexusFlow", RNFLVascularPlexusFlow),
    ("RNFLVascularPlexusMap", RNFLVascularPlexusMap),
    ("RO532344189Zr", RO532344189Zr),
    ("RO54290889Zr", RO54290889Zr),
    ("RO692496311C", RO692496311C),
    ("RO693164311C", RO693164311C),
    ("RO695894818F", RO695894818F),
    ("ROIDepth", ROIDepth),
    ("RPLGD", RPLGD),
    ("RRInterval", RRInterval),
    ("RTBeamEnergy", RTBeamEnergy),
    ("RTBrachyTreatment", RTBrachyTreatment),
    ("RTDose", RTDose),
    ("RTDoseCalculationStructure", RTDoseCalculationStructure),
    ("RTGeometricInformation", RTGeometricInformation),
    ("RTImage", RTImage),
    ("RTImageGuidedPatientPositioningAndTreatmentDelivery", RTImageGuidedPatientPositioningAndTreatmentDelivery),
    ("RTMachineQA", RTMachineQA),
    ("RTPatientPositionAcquisitionCTKV", RTPatientPositionAcquisitionCTKV),
    ("RTPatientPositionAcquisitionCTMV", RTPatientPositionAcquisitionCTMV),
    ("RTPatientPositionAcquisitionConeBeamCTKV", RTPatientPositionAcquisitionConeBeamCTKV),
    ("RTPatientPositionAcquisitionConeBeamCTMV", RTPatientPositionAcquisitionConeBeamCTMV),
    ("RTPatientPositionAcquisitionConventionalCTKV", RTPatientPositionAcquisitionConventionalCTKV),
    ("RTPatientPositionAcquisitionConventionalCTMV", RTPatientPositionAcquisitionConventionalCTMV),
    ("RTPatientPositionAcquisitionDualPlaneKV", RTPatientPositionAcquisitionDualPlaneKV),
    ("RTPatientPositionAcquisitionDualPlaneKVPerMV", RTPatientPositionAcquisitionDualPlaneKVPerMV),
    ("RTPatientPositionAcquisitionDualPlaneMV", RTPatientPositionAcquisitionDualPlaneMV),
    ("RTPatientPositionAcquisitionFilmCassetteKV", RTPatientPositionAcquisitionFilmCassetteKV),
    ("RTPatientPositionAcquisitionFilmCassetteMV", RTPatientPositionAcquisitionFilmCassetteMV),
    ("RTPatientPositionAcquisitionIntegratedDoseMV", RTPatientPositionAcquisitionIntegratedDoseMV),
    ("RTPatientPositionAcquisitionMR", RTPatientPositionAcquisitionMR),
    ("RTPatientPositionAcquisitionOptical", RTPatientPositionAcquisitionOptical),
    ("RTPatientPositionAcquisitionSinglePlaneKV", RTPatientPositionAcquisitionSinglePlaneKV),
    ("RTPatientPositionAcquisitionSinglePlaneMV", RTPatientPositionAcquisitionSinglePlaneMV),
    ("RTPatientPositionAcquisitionSpatialFiducials", RTPatientPositionAcquisitionSpatialFiducials),
    ("RTPatientPositionAcquisitionUltrasound", RTPatientPositionAcquisitionUltrasound),
    ("RTPatientPositionAdjustment", RTPatientPositionAdjustment),
    ("RTPatientPositionInTreatmentSessionReview", RTPatientPositionInTreatmentSessionReview),
    ("RTPatientPositionRegistration2DOn3DReference", RTPatientPositionRegistration2DOn3DReference),
    ("RTPatientPositionRegistration3DCTGeneral", RTPatientPositionRegistration3DCTGeneral),
    ("RTPatientPositionRegistration3DCTMarkerBased", RTPatientPositionRegistration3DCTMarkerBased),
    ("RTPatientPositionRegistration3DCTVolumeBased", RTPatientPositionRegistration3DCTVolumeBased),
    ("RTPatientPositionRegistration3DOn2DReference", RTPatientPositionRegistration3DOn2DReference),
    ("RTPatientPositionRegistrationDualPlane", RTPatientPositionRegistrationDualPlane),
    ("RTPatientPositionRegistrationMR", RTPatientPositionRegistrationMR),
    ("RTPatientPositionRegistrationOptical", RTPatientPositionRegistrationOptical),
    ("RTPatientPositionRegistrationSinglePlane", RTPatientPositionRegistrationSinglePlane),
    ("RTPatientPositionRegistrationSpatialFiducials", RTPatientPositionRegistrationSpatialFiducials),
    ("RTPatientPositionRegistrationUltrasound", RTPatientPositionRegistrationUltrasound),
    ("RTPatientPositioningReferenceImage", RTPatientPositioningReferenceImage),
    ("RTPatientSetup", RTPatientSetup),
    ("RTPlan", RTPlan),
    ("RTPlanLabel", RTPlanLabel),
    ("RTPlanSummationInputUsed", RTPlanSummationInputUsed),
    ("RTPlanningResult", RTPlanningResult),
    ("RTPreTreatmentConsistencyCheck", RTPreTreatmentConsistencyCheck),
    ("RTPreTreatmentDoseCheck", RTPreTreatmentDoseCheck),
    ("RTPrescriptionInputImages", RTPrescriptionInputImages),
    ("RTPrescriptionInputUsed", RTPrescriptionInputUsed),
    ("RTPrescriptionResult", RTPrescriptionResult),
    ("RTRadiationPreviouslyDelivered", RTRadiationPreviouslyDelivered),
    ("RTRegistrationMark", RTRegistrationMark),
    ("RTSegmentMaterial", RTSegmentMaterial),
    ("RTStructureSet", RTStructureSet),
    ("RTTarget", RTTarget),
    ("RTTreatmentCourseSummary", RTTreatmentCourseSummary),
    ("RTTreatmentPlanForThePositionBeingVerified", RTTreatmentPlanForThePositionBeingVerified),
    ("RTTreatmentPlanningInputImages", RTTreatmentPlanningInputImages),
    ("RTTreatmentPlanningInputUsed", RTTreatmentPlanningInputUsed),
    ("RTTreatmentQAByRTIonPlanDifferenceCheck", RTTreatmentQAByRTIonPlanDifferenceCheck),
    ("RTTreatmentQAByRTIonPlanDoseCheck", RTTreatmentQAByRTIonPlanDoseCheck),
    ("RTTreatmentQAByRTPlanDifferenceCheck", RTTreatmentQAByRTPlanDifferenceCheck),
    ("RTTreatmentQAByRTPlanDoseCheck", RTTreatmentQAByRTPlanDoseCheck),
    ("RTTreatmentQAResult", RTTreatmentQAResult),
    ("RTTreatmentQAWithExternalVerification", RTTreatmentQAWithExternalVerification),
    ("RTTreatmentQAWithInternalVerification", RTTreatmentQAWithInternalVerification),
    ("RTTreatmentRecord", RTTreatmentRecord),
    ("RTTreatmentSessionResult", RTTreatmentSessionResult),
    ("RTTreatmentSimulationResult", RTTreatmentSimulationResult),
    ("RTTreatmentSimulationWithExternalVerification", RTTreatmentSimulationWithExternalVerification),
    ("RTTreatmentSimulationWithInternalVerification", RTTreatmentSimulationWithInternalVerification),
    ("RTTreatmentWithAdHocPlanning", RTTreatmentWithAdHocPlanning),
    ("RTTreatmentWithExternalVerification", RTTreatmentWithExternalVerification),
    ("RTTreatmentWithInternalVerification", RTTreatmentWithInternalVerification),
    ("RTVAudioAndStereoVideoRendition", RTVAudioAndStereoVideoRendition),
    ("RTVAudioAndVideoRendition", RTVAudioAndVideoRendition),
    ("RTVRendition", RTVRendition),
    ("RTVStereoVideoRendition", RTVStereoVideoRendition),
    ("RTWorkflowInputUsed", RTWorkflowInputUsed),
    ("RWave", RWave),
    ("RadialBScanPattern", RadialBScanPattern),
    ("RadialBasedWallMotionAnalysis", RadialBasedWallMotionAnalysis),
    ("RadialDiffusivity", RadialDiffusivity),
    ("RadialKurtosis", RadialKurtosis),
    ("RadialLagrangianStrain", RadialLagrangianStrain),
    ("RadialLagrangianStrainRate", RadialLagrangianStrainRate),
    ("RadialPeripapillaryStructuralReflectanceMap", RadialPeripapillaryStructuralReflectanceMap),
    ("RadialPeripapillaryVasculatureFlow", RadialPeripapillaryVasculatureFlow),
    ("RadialScar", RadialScar),
    ("RadialShortening", RadialShortening),
    ("RadialUltrasoundTransducerGeometry", RadialUltrasoundTransducerGeometry),
    ("RadiationApplied", RadiationApplied),
    ("RadiationCharacteristicsNote", RadiationCharacteristicsNote),
    ("RadiationDetectedDuringBeamHoldInterlock", RadiationDetectedDuringBeamHoldInterlock),
    ("RadiationDose", RadiationDose),
    ("RadiationDoseCompositeParameters", RadiationDoseCompositeParameters),
    ("RadiationDoseEstimate", RadiationDoseEstimate),
    ("RadiationDoseEstimateMethod", RadiationDoseEstimateMethod),
    ("RadiationDoseEstimateMethodReference", RadiationDoseEstimateMethodReference),
    ("RadiationDoseEstimateMethodType", RadiationDoseEstimateMethodType),
    ("RadiationDoseEstimateMethodology", RadiationDoseEstimateMethodology),
    ("RadiationDoseEstimateName", RadiationDoseEstimateName),
    ("RadiationDoseEstimateParameterType", RadiationDoseEstimateParameterType),
    ("RadiationDoseEstimateParameters", RadiationDoseEstimateParameters),
    ("RadiationDoseEstimateRepresentation", RadiationDoseEstimateRepresentation),
    ("RadiationDoseNormalizationPoint", RadiationDoseNormalizationPoint),
    ("RadiationDoseReferencePoint", RadiationDoseReferencePoint),
    ("RadiationDoseRepresentationData", RadiationDoseRepresentationData),
    ("RadiationDoseShapingVolume", RadiationDoseShapingVolume),
    ("RadiationExposure", RadiationExposure),
    ("RadiationExposureAndProtectionInformation", RadiationExposureAndProtectionInformation),
    ("RadiationFieldArea", RadiationFieldArea),
    ("RadiationFieldOutline", RadiationFieldOutline),
    ("RadiationInteractionVolume", RadiationInteractionVolume),
    ("RadiationOutput", RadiationOutput),
    ("RadiationPhysicist", RadiationPhysicist),
    ("RadiationRemoved", RadiationRemoved),
    ("RadiationSourceCharacteristics", RadiationSourceCharacteristics),
    ("RadiationTechnique", RadiationTechnique),
    ("RadiationTherapyDevice", RadiationTherapyDevice),
    ("RadiationTherapyParticle", RadiationTherapyParticle),
    ("RadiationTransportBasedMethods", RadiationTransportBasedMethods),
    ("RadiationTransportModelType", RadiationTransportModelType),
    ("RadioFrequencyAblationPower", RadioFrequencyAblationPower),
    ("RadioactiveMarker", RadioactiveMarker),
    ("RadioactiveSubstanceAdministered", RadioactiveSubstanceAdministered),
    ("Radiofluoroscopy", Radiofluoroscopy),
    ("RadiofrequencyTransponder", RadiofrequencyTransponder),
    ("Radiographer", Radiographer),
    ("RadiographicAnatomy", RadiographicAnatomy),
    ("RadiographicImaging", RadiographicImaging),
    ("RadioisotopeGenerator", RadioisotopeGenerator),
    ("RadionuclideResidualSyringeCounts", RadionuclideResidualSyringeCounts),
    ("RadionuclideSyringeCounts", RadionuclideSyringeCounts),
    ("RadionuclideTotalDose", RadionuclideTotalDose),
    ("RadionuclideVialIdentifier", RadionuclideVialIdentifier),
    ("Radiopharmaceutical", Radiopharmaceutical),
    ("RadiopharmaceuticalAdministration", RadiopharmaceuticalAdministration),
    ("RadiopharmaceuticalAdministrationEventUID", RadiopharmaceuticalAdministrationEventUID),
    ("RadiopharmaceuticalDispenseUnitIdentifier", RadiopharmaceuticalDispenseUnitIdentifier),
    ("RadiopharmaceuticalLotIdentifier", RadiopharmaceuticalLotIdentifier),
    ("RadiopharmaceuticalRadiationDoseReport", RadiopharmaceuticalRadiationDoseReport),
    ("RadiopharmaceuticalSpecificActivity", RadiopharmaceuticalSpecificActivity),
    ("RadiopharmaceuticalStartDateTime", RadiopharmaceuticalStartDateTime),
    ("RadiopharmaceuticalStopDateTime", RadiopharmaceuticalStopDateTime),
    ("RadiopharmaceuticalVolume", RadiopharmaceuticalVolume),
    ("RadiotherapyApplicator", RadiotherapyApplicator),
    ("RadiotherapyDoseRegion", RadiotherapyDoseRegion),
    ("RadiotherapyFiducial", RadiotherapyFiducial),
    ("RadiotherapyTreatmentDevice", RadiotherapyTreatmentDevice),
    ("RaisedSkinLesion", RaisedSkinLesion),
    ("Ranibizumab89Zr", Ranibizumab89Zr),
    ("RasterBScanPattern", RasterBScanPattern),
    ("RateOfAdministration", RateOfAdministration),
    ("RateOfExposure", RateOfExposure),
    ("RatingOfPerceivedExertion", RatingOfPerceivedExertion),
    ("RatioOfAchievedToPredictedFunctionalCapacity", RatioOfAchievedToPredictedFunctionalCapacity),
    ("RatioOfAchievedToPredictedMaximalOxygenConsumption", RatioOfAchievedToPredictedMaximalOxygenConsumption),
    ("ReaderSpecialty", ReaderSpecialty),
    ("ReagentVialIdentifier", ReagentVialIdentifier),
    ("RealTimeAcquisition", RealTimeAcquisition),
    ("RealWorldValueMap", RealWorldValueMap),
    ("RealWorldValueMapUsedForMeasurement", RealWorldValueMapUsedForMeasurement),
    ("ReasonForBiosafetyControls", ReasonForBiosafetyControls),
    ("ReasonForProcedure", ReasonForProcedure),
    ("ReasonForProceeding", ReasonForProceeding),
    ("ReasonForRejectingAcquisition", ReasonForRejectingAcquisition),
    ("ReasonForRepeatingAcquisition", ReasonForRepeatingAcquisition),
    ("ReasonForStudy", ReasonForStudy),
    ("RecallForImagingFindings", RecallForImagingFindings),
    ("RecallForPatientSymptomsPerClinicalFindings", RecallForPatientSymptomsPerClinicalFindings),
    ("RecallForTechnicalReasons", RecallForTechnicalReasons),
    ("RecentPhysicalActivity", RecentPhysicalActivity),
    ("ReciprocatingGrid", ReciprocatingGrid),
    ("Recommendation", Recommendation),
    ("Recommendations", Recommendations),
    ("RecommendedCADOperatingPoint", RecommendedCADOperatingPoint),
    ("RecommendedFollowUp", RecommendedFollowUp),
    ("RecommendedFollowUpDate", RecommendedFollowUpDate),
    ("RecommendedFollowUpInterval", RecommendedFollowUpInterval),
    ("Reconstructed", Reconstructed),
    ("ReconstructionAlgorithm", ReconstructionAlgorithm),
    ("Recording", Recording),
    ("RecoveryState", RecoveryState),
    ("RectalBalloon", RectalBalloon),
    ("RecumbentPatientPositionWithRespectToGravity", RecumbentPatientPositionWithRespectToGravity),
    ("RecurrentMalignancy", RecurrentMalignancy),
    ("RedFilter", RedFilter),
    ("RedTranslucentIgloo", RedTranslucentIgloo),
    ("RedistributionState", RedistributionState),
    ("ReferenceAcquisitionPoint", ReferenceAcquisitionPoint),
    ("ReferenceAuthority", ReferenceAuthority),
    ("ReferenceBasis", ReferenceBasis),
    ("ReferenceEnergy", ReferenceEnergy),
    ("ReferenceGeometry", ReferenceGeometry),
    ("ReferenceMAs", ReferenceMAs),
    ("ReferenceMeasurementGroup", ReferenceMeasurementGroup),
    ("ReferenceMethod", ReferenceMethod),
    ("ReferencePoint", ReferencePoint),
    ("ReferencePointDefinition", ReferencePointDefinition),
    ("ReferencePointDosimetry", ReferencePointDosimetry),
    ("ReferencePointPosition", ReferencePointPosition),
    ("ReferencePoints", ReferencePoints),
    ("ReferenceToUncertaintyDeterminationMethod", ReferenceToUncertaintyDeterminationMethod),
    ("ReferencedBeamNumber", ReferencedBeamNumber),
    ("ReferencedFrames", ReferencedFrames),
    ("ReferencedImagingAgentAdministrationPhaseIdentifier", ReferencedImagingAgentAdministrationPhaseIdentifier),
    ("ReferencedImagingAgentAdministrationPhaseUID", ReferencedImagingAgentAdministrationPhaseUID),
    ("ReferencedImagingAgentAdministrationStepUID", ReferencedImagingAgentAdministrationStepUID),
    ("ReferencedImagingAgentIdentifier", ReferencedImagingAgentIdentifier),
    ("ReferencedPatientAlignmentReference", ReferencedPatientAlignmentReference),
    ("ReferencedRegionOfInterestIdentifier", ReferencedRegionOfInterestIdentifier),
    ("ReferencedSegment", ReferencedSegment),
    ("ReferencedSegmentationFrame", ReferencedSegmentationFrame),
    ("Referring", Referring),
    ("ReflectionBasedCornealTopographer", ReflectionBasedCornealTopographer),
    ("ReflectionIllumination", ReflectionIllumination),
    ("RefractiveMeasurementsSOPInstance", RefractiveMeasurementsSOPInstance),
    ("RegionInSpace", RegionInSpace),
    ("RegionalAbnormalWallMotion", RegionalAbnormalWallMotion),
    ("RegionalCerebralBloodFlow", RegionalCerebralBloodFlow),
    ("RegionalCerebralBloodVolume", RegionalCerebralBloodVolume),
    ("RegionalContributionToEjectionFraction", RegionalContributionToEjectionFraction),
    ("Registrar", Registrar),
    ("Registration", Registration),
    ("RegistrationCreatedDuringTreatment", RegistrationCreatedDuringTreatment),
    ("RegistrationInput", RegistrationInput),
    ("RegistrationMethod", RegistrationMethod),
    ("RegistrationResult", RegistrationResult),
    ("RegistrationUsedInPlanning", RegistrationUsedInPlanning),
    ("RegressionOffsetED", RegressionOffsetED),
    ("RegressionOffsetES", RegressionOffsetES),
    ("RegressionSlopeED", RegressionSlopeED),
    ("RegressionSlopeES", RegressionSlopeES),
    ("RegressionVolumeExponent", RegressionVolumeExponent),
    ("RegurgitationJetArea", RegurgitationJetArea),
    ("RegurgitationJetWidth", RegurgitationJetWidth),
    ("ReinjectionState", ReinjectionState),
    ("RejectedForPatientSafetyReasons", RejectedForPatientSafetyReasons),
    ("RejectedForQualityReasons", RejectedForQualityReasons),
    ("RejectionWithdrawn", RejectionWithdrawn),
    ("RelatedImplantationReports", RelatedImplantationReports),
    ("RelatedOphthalmicThicknessMap", RelatedOphthalmicThicknessMap),
    ("RelatedPatientDataNotUsedDuringPlanning", RelatedPatientDataNotUsedDuringPlanning),
    ("RelatedToNonMacularSensitivity", RelatedToNonMacularSensitivity),
    ("RelativeAmountOfExposure", RelativeAmountOfExposure),
    ("RelativeAmountOfUse", RelativeAmountOfUse),
    ("RelativeAnisotropy", RelativeAnisotropy),
    ("RelativeDoseAmount", RelativeDoseAmount),
    ("RelativeDoseFrequency", RelativeDoseFrequency),
    ("RelativeElectronDensity", RelativeElectronDensity),
    ("RelativeFrequencyOfExposure", RelativeFrequencyOfExposure),
    ("RelativeFrequencyOfUse", RelativeFrequencyOfUse),
    ("RelativeLinearStoppingPower", RelativeLinearStoppingPower),
    ("RelativeMassDensity", RelativeMassDensity),
    ("RelativePosition", RelativePosition),
    ("RelativeRegionalBloodFlow", RelativeRegionalBloodFlow),
    ("RelativeRegionalBloodVolume", RelativeRegionalBloodVolume),
    ("RelativeStenosisSeverity", RelativeStenosisSeverity),
    ("RelevantIndicatedProblems", RelevantIndicatedProblems),
    ("RelevantPatientInformation", RelevantPatientInformation),
    ("RelevantPatientInformationForBreastImaging", RelevantPatientInformationForBreastImaging),
    ("RelevantPreviousProcedures", RelevantPreviousProcedures),
    ("RelevantRiskFactors", RelevantRiskFactors),
    ("RemainingConsumableDisposed", RemainingConsumableDisposed),
    ("RemodelingIndex", RemodelingIndex),
    ("RemoteServiceOperationStarted", RemoteServiceOperationStarted),
    ("RemoteServiceOperationStopped", RemoteServiceOperationStopped),
    ("RenderingIntent", RenderingIntent),
    ("ReplacedByAlternateRepresentation", ReplacedByAlternateRepresentation),
    ("ReplacedReport", ReplacedReport),
    ("ReportRequested", ReportRequested),
    ("ReportVerification", ReportVerification),
    ("ReportedValueType", ReportedValueType),
    ("ReportingSystem", ReportingSystem),
    ("RepositionedROIOrGrid", RepositionedROIOrGrid),
    ("RepresentativeOfEthicsCommittee", RepresentativeOfEthicsCommittee),
    ("RepresentativeOfProtocolCommittee", RepresentativeOfProtocolCommittee),
    ("RepresentativePoint", RepresentativePoint),
    ("Request", Request),
    ("Requesting", Requesting),
    ("Resident", Resident),
    ("ResidualVolumeOfImagingAgentInContainer", ResidualVolumeOfImagingAgentInContainer),
    ("ResourceInadequate", ResourceInadequate),
    ("ResourcePreEmpted", ResourcePreEmpted),
    ("RespirationWaveform", RespirationWaveform),
    ("Respiratory", Respiratory),
    ("RespiratoryEffort", RespiratoryEffort),
    ("RespiratoryPressure", RespiratoryPressure),
    ("ResponseEvaluation", ResponseEvaluation),
    ("ResponseEvaluationMethod", ResponseEvaluationMethod),
    ("RestTechnetiumPerStressTechnetium1DayProcedure", RestTechnetiumPerStressTechnetium1DayProcedure),
    ("RestTechnetiumPerStressTechnetium2DayProcedure", RestTechnetiumPerStressTechnetium2DayProcedure),
    ("RestThalliumPerStressTechnetiumProcedure", RestThalliumPerStressTechnetiumProcedure),
    ("RestenoticLesion", RestenoticLesion),
    ("ResultsCommunicated", ResultsCommunicated),
    ("ResultsQualityController", ResultsQualityController),
    ("ResumeProcedureAction", ResumeProcedureAction),
    ("RetainDeviceIdentityOption", RetainDeviceIdentityOption),
    ("RetainInstitutionIdentityOption", RetainInstitutionIdentityOption),
    ("RetainLongitudinalTemporalInformationFullDatesOption", RetainLongitudinalTemporalInformationFullDatesOption),
    ("RetainLongitudinalTemporalInformationModifiedDatesOption", RetainLongitudinalTemporalInformationModifiedDatesOption),
    ("RetainPatientCharacteristicsOption", RetainPatientCharacteristicsOption),
    ("RetainSafePrivateOption", RetainSafePrivateOption),
    ("RetainUIDsOption", RetainUIDsOption),
    ("Reticle", Reticle),
    ("ReticularPattern", ReticularPattern),
    ("ReticulonodularPattern", ReticulonodularPattern),
    ("RetinaDepthEncodedStructuralReflectanceMap", RetinaDepthEncodedStructuralReflectanceMap),
    ("RetinaDepthEncodedVasculatureFlow", RetinaDepthEncodedVasculatureFlow),
    ("RetinaStructuralReflectanceMap", RetinaStructuralReflectanceMap),
    ("RetinaVasculatureFlow", RetinaVasculatureFlow),
    ("RetinalNerveFiberLayerSymmetry", RetinalNerveFiberLayerSymmetry),
    ("RetinalNerveFiberLayerThickness", RetinalNerveFiberLayerThickness),
    ("RetinalROIHeight", RetinalROIHeight),
    ("RetinalROIWidth", RetinalROIWidth),
    ("RetinalTopography", RetinalTopography),
    ("RetroOrbitalRoute", RetroOrbitalRoute),
    ("RetroPectoralImplant", RetroPectoralImplant),
    ("RetronasalTriangle", RetronasalTriangle),
    ("RetrospectiveGating", RetrospectiveGating),
    ("Reused", Reused),
    ("ReverseOsmosisPurifiedHClAcidifiedWater", ReverseOsmosisPurifiedHClAcidifiedWater),
    ("ReverseOsmosisPurifiedWater", ReverseOsmosisPurifiedWater),
    ("ReviewOfAnOutsideStudy", ReviewOfAnOutsideStudy),
    ("RibScaleneTubercle", RibScaleneTubercle),
    ("RightArmRestPitchAngle", RightArmRestPitchAngle),
    ("RightArmRestPosition", RightArmRestPosition),
    ("RightArmRestRollAngle", RightArmRestRollAngle),
    ("RightContour", RightContour),
    ("RightCoronaryArteryOstiumHeight", RightCoronaryArteryOstiumHeight),
    ("RightEyeRx", RightEyeRx),
    ("RightFirst", RightFirst),
    ("RightHemisphereMostAnterior", RightHemisphereMostAnterior),
    ("RightHemisphereMostInferior", RightHemisphereMostInferior),
    ("RightHemisphereMostPosterior", RightHemisphereMostPosterior),
    ("RightHemisphereMostSuperior", RightHemisphereMostSuperior),
    ("RightToLeft", RightToLeft),
    ("RightToLeftFlow", RightToLeftFlow),
    ("RightTrachealStripe", RightTrachealStripe),
    ("RightVentricleAnteriorWall", RightVentricleAnteriorWall),
    ("RightVentricleCardiacOutput", RightVentricleCardiacOutput),
    ("RightVentricleDiastolicMajorAxis", RightVentricleDiastolicMajorAxis),
    ("RightVentricleOutflowTractDistal", RightVentricleOutflowTractDistal),
    ("RightVentricleOutflowTractProximal", RightVentricleOutflowTractProximal),
    ("RightVentricularDiastolicBasalMinorAxis", RightVentricularDiastolicBasalMinorAxis),
    ("RightVentricularDiastolicMidSegmentMinorAxis", RightVentricularDiastolicMidSegmentMinorAxis),
    ("RightVentricularEjectionTime", RightVentricularEjectionTime),
    ("RightVentricularFillingTime", RightVentricularFillingTime),
    ("RightVentricularInletLength", RightVentricularInletLength),
    ("RingUltrasoundTransducerGeometry", RingUltrasoundTransducerGeometry),
    ("RiseTime", RiseTime),
    ("RiskFactorModifier", RiskFactorModifier),
    ("Rituximab89Zr", Rituximab89Zr),
    ("RobertsonAndReidTable", RobertsonAndReidTable),
    ("RoleOfPersonReporting", RoleOfPersonReporting),
    ("Roledumab89Zr", Roledumab89Zr),
    ("RoomBarometricPressure", RoomBarometricPressure),
    ("RoomIdentification", RoomIdentification),
    ("RoomLaserPatientSetupPoint", RoomLaserPatientSetupPoint),
    ("RoomOrigin", RoomOrigin),
    ("RoomOxygenConcentration", RoomOxygenConcentration),
    ("RoomTemperature", RoomTemperature),
    ("RootAngularSecondMomentOfGLCM", RootAngularSecondMomentOfGLCM),
    ("RotationAngle", RotationAngle),
    ("RotationPlaneNormalPoint", RotationPlaneNormalPoint),
    ("RotationalAcquisition", RotationalAcquisition),
    ("RunEntropy", RunEntropy),
    ("RunLengthNonuniformity", RunLengthNonuniformity),
    ("RunLengthNonuniformityNormalized", RunLengthNonuniformityNormalized),
    ("RunLengthVariance", RunLengthVariance),
    ("RunPercentage", RunPercentage),
    ("RungeKutta", RungeKutta),
    ("SAR341989Zr", SAR341989Zr),
    ("SMILE", SMILE),
    ("SMPTEPattern", SMPTEPattern),
    ("SNITRectangularSectors", SNITRectangularSectors),
    ("SOPClassUID", SOPClassUID),
    ("SPECTCTOfWholeBody", SPECTCTOfWholeBody),
    ("SPECTOfWholeBody", SPECTOfWholeBody),
    ("SPhase", SPhase),
    ("SRInstanceUsed", SRInstanceUsed),
    ("SRKII", SRKII),
    ("SRKT", SRKT),
    ("SSDEConversionFactor", SSDEConversionFactor),
    ("SStent", SStent),
    ("STChangeFromBaseline", STChangeFromBaseline),
    ("STDepressionDownsloping", STDepressionDownsloping),
    ("STDepressionHorizontal", STDepressionHorizontal),
    ("STDepressionUpsloping", STDepressionUpsloping),
    ("STElevationOnsetDateTime", STElevationOnsetDateTime),
    ("STPerHRIndex", STPerHRIndex),
    ("SUVBodySurfaceAreaCalculationMethod", SUVBodySurfaceAreaCalculationMethod),
    ("SUVBodyWeightCalculationMethod", SUVBodyWeightCalculationMethod),
    ("SUVIdealBodyWeightCalculationMethod", SUVIdealBodyWeightCalculationMethod),
    ("SUVLeanBodyMassCalculationJanmahasatianMethod", SUVLeanBodyMassCalculationJanmahasatianMethod),
    ("SUVLeanBodyMassCalculationMethod", SUVLeanBodyMassCalculationMethod),
    ("SUVLeanBodyMassCalculationMethodUsing128Multiplier", SUVLeanBodyMassCalculationMethodUsing128Multiplier),
    ("SUVbsa", SUVbsa),
    ("SUVbw", SUVbw),
    ("SUVibw", SUVibw),
    ("SUVlbm", SUVlbm),
    ("SUVlbmJames128", SUVlbmJames128),
    ("SUVlbmJanma", SUVlbmJanma),
    ("SWave", SWave),
    ("Saliency", Saliency),
    ("SalineImplant", SalineImplant),
    ("SameAcquisitionAtDifferentFocalDepth", SameAcquisitionAtDifferentFocalDepth),
    ("SameAcquisitionAtDifferentSpectralBand", SameAcquisitionAtDifferentSpectralBand),
    ("SameAcquisitionAtHigherResolution", SameAcquisitionAtHigherResolution),
    ("SameAcquisitionAtLowerResolution", SameAcquisitionAtLowerResolution),
    ("SameAnatomy", SameAnatomy),
    ("SameIndication", SameIndication),
    ("SampleVolumeDepth", SampleVolumeDepth),
    ("SamplingDateTime", SamplingDateTime),
    ("SamplingFrequency", SamplingFrequency),
    ("SamplingMethod", SamplingMethod),
    ("ScanDelay", ScanDelay),
    ("ScanSpotInterlock", ScanSpotInterlock),
    ("ScanSpotMetersetInterlock", ScanSpotMetersetInterlock),
    ("ScanSpotPositionInterlock", ScanSpotPositionInterlock),
    ("ScanSpotSizeInterlock", ScanSpotSizeInterlock),
    ("ScanningLength", ScanningLength),
    ("ScapularInfraspinatusFossa", ScapularInfraspinatusFossa),
    ("ScapularSpine", ScapularSpine),
    ("ScapularSupraspinatusFossa", ScapularSupraspinatusFossa),
    ("ScarTissue", ScarTissue),
    ("ScarredMyocardium", ScarredMyocardium),
    ("ScheimpflugCamera", ScheimpflugCamera),
    ("Schneider2005", Schneider2005),
    ("SclerosingAdenosis", SclerosingAdenosis),
    ("ScopeOfAccumulation", ScopeOfAccumulation),
    ("ScopeOfFeature", ScopeOfFeature),
    ("ScotopicLight", ScotopicLight),
    ("Scratch", Scratch),
    ("ScrubNurse", ScrubNurse),
    ("SeatPan", SeatPan),
    ("SeatPanHeight", SeatPanHeight),
    ("SeatPanPitchAngle", SeatPanPitchAngle),
    ("SecondMeasureOfInformationCorrelationOfGLCM", SecondMeasureOfInformationCorrelationOfGLCM),
    ("SecondaryFluenceMonitoringSystemInterlock", SecondaryFluenceMonitoringSystemInterlock),
    ("SecondaryFollicle", SecondaryFollicle),
    ("SecondaryPulmonaryLobule", SecondaryPulmonaryLobule),
    ("SectorUltrasoundTransducerGeometry", SectorUltrasoundTransducerGeometry),
    ("SecureDigitalCard", SecureDigitalCard),
    ("SecurityAlert", SecurityAlert),
    ("SecurityConfiguration", SecurityConfiguration),
    ("SecurityRolesChanged", SecurityRolesChanged),
    ("SeedPoint", SeedPoint),
    ("SegmentationMethod", SegmentationMethod),
    ("SegmentationResult", SegmentationResult),
    ("Segmentation_113076", Segmentation_113076),
    ("Segmentation_SEG", Segmentation_SEG),
    ("SegmentedConstrained", SegmentedConstrained),
    ("SegmentedUnconstrained", SegmentedUnconstrained),
    ("SelectedImplantComponent", SelectedImplantComponent),
    ("SelectedRegion", SelectedRegion),
    ("SelectedRegionDescription", SelectedRegionDescription),
    ("SelectionStatus", SelectionStatus),
    ("SemicircularSectors", SemicircularSectors),
    ("SensitiveContent", SensitiveContent),
    ("SentForAnalysis", SentForAnalysis),
    ("SeptalDefectIntervention", SeptalDefectIntervention),
    ("SeptalLineS", SeptalLineS),
    ("SeptumToWall", SeptumToWall),
    ("SeptumWallAxis", SeptumWallAxis),
    ("SequencedAcquisition", SequencedAcquisition),
    ("Series", Series),
    ("SeriesDate", SeriesDate),
    ("SeriesDescription", SeriesDescription),
    ("SeriesInstanceUID", SeriesInstanceUID),
    ("SeriesNumber", SeriesNumber),
    ("SeriesOrInstanceUsedForWaterEquivalentDiameterEstimation", SeriesOrInstanceUsedForWaterEquivalentDiameterEstimation),
    ("SeriesTime", SeriesTime),
    ("ServiceDeliveryLocation", ServiceDeliveryLocation),
    ("ServicePerformer", ServicePerformer),
    ("SessionStart", SessionStart),
    ("SessionStop", SessionStop),
    ("SevereDistortionInTheAreaOfInterest", SevereDistortionInTheAreaOfInterest),
    ("SeverelyJaundiced", SeverelyJaundiced),
    ("SeverityOfCalcification", SeverityOfCalcification),
    ("SeverityOfComplication", SeverityOfComplication),
    ("Sewing", Sewing),
    ("SexOfAnimalsWithinSameHousingUnit", SexOfAnimalsWithinSameHousingUnit),
    ("SexOfHandler", SexOfHandler),
    ("Shadow", Shadow),
    ("ShammasNoHistory", ShammasNoHistory),
    ("ShapeFromMotion", ShapeFromMotion),
    ("ShapeFromShading", ShapeFromShading),
    ("SharplyDefined", SharplyDefined),
    ("SharplyDemarcated", SharplyDemarcated),
    ("ShearWaveDetectionMethod", ShearWaveDetectionMethod),
    ("ShearWaveDispersionSlope", ShearWaveDispersionSlope),
    ("ShearWaveDispersionSlopeCenterFrequency", ShearWaveDispersionSlopeCenterFrequency),
    ("ShearWaveSpeed", ShearWaveSpeed),
    ("SheetFilmDigitized", SheetFilmDigitized),
    ("ShelterManufacturer", ShelterManufacturer),
    ("ShelterProductCode", ShelterProductCode),
    ("ShelterProductName", ShelterProductName),
    ("ShelterType", ShelterType),
    ("ShinRest", ShinRest),
    ("ShinRestPosition", ShinRestPosition),
    ("ShortAxisViewAtPulmonaryArteryLevel", ShortAxisViewAtPulmonaryArteryLevel),
    ("ShortLabel", ShortLabel),
    ("ShortRunHighGrayLevelEmphasis", ShortRunHighGrayLevelEmphasis),
    ("ShortRunLowGrayLevelEmphasis", ShortRunLowGrayLevelEmphasis),
    ("ShortRunsEmphasis", ShortRunsEmphasis),
    ("ShutterSpeedModel", ShutterSpeedModel),
    ("SideOfFamily", SideOfFamily),
    ("SignalBaselineEnd", SignalBaselineEnd),
    ("SignalBaselineStart", SignalBaselineStart),
    ("SignalChange", SignalChange),
    ("SignalEarliestPeakTime", SignalEarliestPeakTime),
    ("SignalIncreaseStartTime", SignalIncreaseStartTime),
    ("SignalTimeToPeak", SignalTimeToPeak),
    ("SignalToNoise", SignalToNoise),
    ("SignalToNoiseRatio", SignalToNoiseRatio),
    ("SignedCompleteAcquisitionContent", SignedCompleteAcquisitionContent),
    ("SignedCompleteStudyContent", SignedCompleteStudyContent),
    ("SignedManifest", SignedManifest),
    ("SignetRingSign", SignetRingSign),
    ("SilhouetteSign", SilhouetteSign),
    ("SiliconeGranuloma", SiliconeGranuloma),
    ("SiliconeLadenLymphNodes", SiliconeLadenLymphNodes),
    ("SimpleObjectModel", SimpleObjectModel),
    ("SimplifiedBernoulli", SimplifiedBernoulli),
    ("SimulatedKeratometry", SimulatedKeratometry),
    ("SimulationIntent", SimulationIntent),
    ("SimulationNote", SimulationNote),
    ("SimultaneousArterialPulseWaveform", SimultaneousArterialPulseWaveform),
    ("SimultaneousDoppler", SimultaneousDoppler),
    ("SimultaneousECG", SimultaneousECG),
    ("SimultaneousHemodynamic", SimultaneousHemodynamic),
    ("SimultaneousPhonocardiographicWaveform", SimultaneousPhonocardiographicWaveform),
    ("SimultaneousRespiratoryWaveform", SimultaneousRespiratoryWaveform),
    ("SimultaneousVoiceNarrative", SimultaneousVoiceNarrative),
    ("SimultaneouslyAcquired", SimultaneouslyAcquired),
    ("SingleFraction", SingleFraction),
    ("SingleImageFinding", SingleImageFinding),
    ("SingleImageFindingModifier", SingleImageFindingModifier),
    ("SingleLADRegionInRAOProjection", SingleLADRegionInRAOProjection),
    ("SingleLeaves", SingleLeaves),
    ("SingleLuminance", SingleLuminance),
    ("SingleOrAnteriorLens", SingleOrAnteriorLens),
    ("SinglePhotonEmissionComputedTomography", SinglePhotonEmissionComputedTomography),
    ("SinglePlane", SinglePlane),
    ("SinglePlaneAngiography", SinglePlaneAngiography),
    ("SinglePlaneEllipse", SinglePlaneEllipse),
    ("SingleRCARegionInRAOProjection", SingleRCARegionInRAOProjection),
    ("SingleShotEPI", SingleShotEPI),
    ("SingleSideIllumination", SingleSideIllumination),
    ("SingleSubjectExtractedFromGroup", SingleSubjectExtractedFromGroup),
    ("SingleTensor", SingleTensor),
    ("SingularValueDecompositionDeconvolution", SingularValueDecompositionDeconvolution),
    ("SiteInvolvement", SiteInvolvement),
    ("SiteOfLumenMaximum", SiteOfLumenMaximum),
    ("SiteOfLuminalMaximum", SiteOfLuminalMaximum),
    ("SiteOfLuminalMinimum", SiteOfLuminalMinimum),
    ("SizeDescriptor", SizeDescriptor),
    ("SizeSpecificDoseEstimate", SizeSpecificDoseEstimate),
    ("SizeSpecificDoseEstimateAtLongitudinalPositionZ", SizeSpecificDoseEstimateAtLongitudinalPositionZ),
    ("Skewness", Skewness),
    ("SkinAppositionSetupMethod", SkinAppositionSetupMethod),
    ("SkinConditionClammy", SkinConditionClammy),
    ("SkinConditionCold", SkinConditionCold),
    ("SkinConditionCool", SkinConditionCool),
    ("SkinConditionDiaphoretic", SkinConditionDiaphoretic),
    ("SkinConditionDry", SkinConditionDry),
    ("SkinConditionFlush", SkinConditionFlush),
    ("SkinConditionMottled", SkinConditionMottled),
    ("SkinConditionPale", SkinConditionPale),
    ("SkinConditionWarm", SkinConditionWarm),
    ("SkinDoseMap", SkinDoseMap),
    ("SkinLesionColor", SkinLesionColor),
    ("SkinOfAnteriorHelixOfEar", SkinOfAnteriorHelixOfEar),
    ("SkinOfCaruncleOfEye", SkinOfCaruncleOfEye),
    ("SkinOfInferiorHelixOfEar", SkinOfInferiorHelixOfEar),
    ("SkinOfInferiorPosteriorSurfaceOfThePinna", SkinOfInferiorPosteriorSurfaceOfThePinna),
    ("SkinOfInfraalarGroove", SkinOfInfraalarGroove),
    ("SkinOfLateralPartOfDorsumOfFoot", SkinOfLateralPartOfDorsumOfFoot),
    ("SkinOfLowerAntihelixOfEar", SkinOfLowerAntihelixOfEar),
    ("SkinOfLowerEyelidMargin", SkinOfLowerEyelidMargin),
    ("SkinOfLowerParaspinalRegion", SkinOfLowerParaspinalRegion),
    ("SkinOfMedialPartOfDorsumOfFoot", SkinOfMedialPartOfDorsumOfFoot),
    ("SkinOfMidBack", SkinOfMidBack),
    ("SkinOfMidParaspinalRegion", SkinOfMidParaspinalRegion),
    ("SkinOfParanasalCheek", SkinOfParanasalCheek),
    ("SkinOfParaspinalAreaOfTheNeck", SkinOfParaspinalAreaOfTheNeck),
    ("SkinOfParaspinalAreaOfTheSuperiorBack", SkinOfParaspinalAreaOfTheSuperiorBack),
    ("SkinOfPosteriorHelixOfEar", SkinOfPosteriorHelixOfEar),
    ("SkinOfPosteriorLobuleOfTheEar", SkinOfPosteriorLobuleOfTheEar),
    ("SkinOfSoleOfForefoot", SkinOfSoleOfForefoot),
    ("SkinOfSuperiorAntihelixOfEar", SkinOfSuperiorAntihelixOfEar),
    ("SkinOfSuperiorPosteriorHelixOfEar", SkinOfSuperiorPosteriorHelixOfEar),
    ("SkinOfSuperiorPosteriorSurfaceOfThePinna", SkinOfSuperiorPosteriorSurfaceOfThePinna),
    ("SkinOfUpperAntihelixOfEar", SkinOfUpperAntihelixOfEar),
    ("SkinOfUpperEyelidMargin", SkinOfUpperEyelidMargin),
    ("SkinOfUpperParaspinalRegion", SkinOfUpperParaspinalRegion),
    ("SleepStageN1", SleepStageN1),
    ("SleepStageN2", SleepStageN2),
    ("SleepStageN3", SleepStageN3),
    ("SliceContributionToEjectionFraction", SliceContributionToEjectionFraction),
    ("SliceThickness", SliceThickness),
    ("SlideIdentifier", SlideIdentifier),
    ("SlideMarking", SlideMarking),
    ("SlideMicroscopy_112702", SlideMicroscopy_112702),
    ("SlideMicroscopy_SM", SlideMicroscopy_SM),
    ("SlideUID", SlideUID),
    ("SlidingWindowBeam", SlidingWindowBeam),
    ("SlowDiffusionCoefficient", SlowDiffusionCoefficient),
    ("SmallIrregularOpacities", SmallIrregularOpacities),
    ("SmallLessThan320CmLateralThickness", SmallLessThan320CmLateralThickness),
    ("SmallRoundedOpacities", SmallRoundedOpacities),
    ("SmallZoneEmphasis", SmallZoneEmphasis),
    ("SmallZoneHighGrayLevelEmphasis", SmallZoneHighGrayLevelEmphasis),
    ("SmallZoneLowGrayLevelEmphasis", SmallZoneLowGrayLevelEmphasis),
    ("Smoothing", Smoothing),
    ("SoftPlaque", SoftPlaque),
    ("SoftTissue", SoftTissue),
    ("SoftwareConfiguration", SoftwareConfiguration),
    ("SoftwareFailure", SoftwareFailure),
    ("SolidMass", SolidMass),
    ("Sonazoid", Sonazoid),
    ("SonoVue", SonoVue),
    ("Sonographer", Sonographer),
    ("Sonologist", Sonologist),
    ("SourceContinuousRollAngle", SourceContinuousRollAngle),
    ("SourceDeformableSpatialRegistration", SourceDeformableSpatialRegistration),
    ("SourceDocument", SourceDocument),
    ("SourceDoseForComposingCurrentDose", SourceDoseForComposingCurrentDose),
    ("SourceImage", SourceImage),
    ("SourceImageDiffusionBValue", SourceImageDiffusionBValue),
    ("SourceImageForImageProcessingOperation", SourceImageForImageProcessingOperation),
    ("SourceImageForMontage", SourceImageForMontage),
    ("SourceImageForSegmentation", SourceImageForSegmentation),
    ("SourceMeasurement", SourceMeasurement),
    ("SourceMedia", SourceMedia),
    ("SourceOfDoseInformation", SourceOfDoseInformation),
    ("SourceOfMeasurement", SourceOfMeasurement),
    ("SourcePositionToleranceViolation", SourcePositionToleranceViolation),
    ("SourceRawData", SourceRawData),
    ("SourceRealWorldValueMap", SourceRealWorldValueMap),
    ("SourceReport", SourceReport),
    ("SourceRoleID", SourceRoleID),
    ("SourceSegmentation", SourceSegmentation),
    ("SourceSeriesForImageProcessingOperation", SourceSeriesForImageProcessingOperation),
    ("SourceSeriesForSegmentation", SourceSeriesForSegmentation),
    ("SpacingBetweenSlices", SpacingBetweenSlices),
    ("SpatialDisplacementXComponent", SpatialDisplacementXComponent),
    ("SpatialDisplacementYComponent", SpatialDisplacementYComponent),
    ("SpatialDisplacementZComponent", SpatialDisplacementZComponent),
    ("SpatialFiducials_128447", SpatialFiducials_128447),
    ("SpatialFiducials_FID", SpatialFiducials_FID),
    ("SpatialRegistration", SpatialRegistration),
    ("SpatialRegistrationReference", SpatialRegistrationReference),
    ("SpatialResampling", SpatialResampling),
    ("SpatiallyRelatedFramesExtractedFromTheVolume", SpatiallyRelatedFramesExtractedFromTheVolume),
    ("SpatiallyTransformedDose", SpatiallyTransformedDose),
    ("SpecialProcedureNote", SpecialProcedureNote),
    ("SpecificAbsorptionRate", SpecificAbsorptionRate),
    ("SpecificObservationSubjectClass", SpecificObservationSubjectClass),
    ("Specified", Specified),
    ("SpecifiedConformationNumber", SpecifiedConformationNumber),
    ("SpecifiedConformityIndex", SpecifiedConformityIndex),
    ("SpecifiedHealthyTissueConformityIndex", SpecifiedHealthyTissueConformityIndex),
    ("SpecifiedHomogeneityIndex", SpecifiedHomogeneityIndex),
    ("SpecifiedRadiationDose", SpecifiedRadiationDose),
    ("SpecifiedVolumePercentage", SpecifiedVolumePercentage),
    ("SpecifiedVolumeSize", SpecifiedVolumeSize),
    ("Specimen", Specimen),
    ("SpecimenAccessionNumber", SpecimenAccessionNumber),
    ("SpecimenContainerIdentifier", SpecimenContainerIdentifier),
    ("SpecimenIdentifier", SpecimenIdentifier),
    ("SpecimenStorage", SpecimenStorage),
    ("SpecimenType", SpecimenType),
    ("SpecimenUID", SpecimenUID),
    ("SpectaclePrescriptionReport", SpectaclePrescriptionReport),
    ("SpectralDomain", SpectralDomain),
    ("SpectralFilteredImage", SpectralFilteredImage),
    ("Spectroscopy", Spectroscopy),
    ("SpectroscopyDataForWaterPhaseCorrection", SpectroscopyDataForWaterPhaseCorrection),
    ("SpeedOfSound", SpeedOfSound),
    ("SpeedOfSoundMapCorrection", SpeedOfSoundMapCorrection),
    ("Sphere", Sphere),
    ("SphericalBackProjection", SphericalBackProjection),
    ("SphericalDeconvolution", SphericalDeconvolution),
    ("SphericalProjection", SphericalProjection),
    ("Spiculated", Spiculated),
    ("SpinTaggingPerfusionMRSignalIntensity", SpinTaggingPerfusionMRSignalIntensity),
    ("SpontaneousAbortion", SpontaneousAbortion),
    ("SpotMagnificationViewS", SpotMagnificationViewS),
    ("SquamousCellCarcinomaOfTheNipple", SquamousCellCarcinomaOfTheNipple),
    ("Stage0", Stage0),
    ("StageI", StageI),
    ("StageIIA", StageIIA),
    ("StageIIB", StageIIB),
    ("StageIIIA", StageIIIA),
    ("StageIIIB", StageIIIB),
    ("StageIIIC", StageIIIC),
    ("StageIV", StageIV),
    ("StageView", StageView),
    ("StagedMeasurements", StagedMeasurements),
    ("StagingPriorToImaging", StagingPriorToImaging),
    ("StandardDeviation", StandardDeviation),
    ("StandardDeviationDistanceOfCalcificationsInACluster", StandardDeviationDistanceOfCalcificationsInACluster),
    ("StandardDeviationOfAttenuationCoefficient", StandardDeviationOfAttenuationCoefficient),
    ("StandardDeviationOfMeasurementsUsed", StandardDeviationOfMeasurementsUsed),
    ("StandardDeviationOfPopulation", StandardDeviationOfPopulation),
    ("StandardDeviationOfUltrasoundAttenuationCoefficient", StandardDeviationOfUltrasoundAttenuationCoefficient),
    ("StandardFractionation", StandardFractionation),
    ("StandardToftsModel", StandardToftsModel),
    ("StandardizedAddedMetabolicActivity", StandardizedAddedMetabolicActivity),
    ("StandardizedAddedMetabolicActivityBackground", StandardizedAddedMetabolicActivityBackground),
    ("StandardizedUptakeValue", StandardizedUptakeValue),
    ("Standby", Standby),
    ("StartOfAtrialContraction", StartOfAtrialContraction),
    ("StartOfAtrialContractionSubsequent", StartOfAtrialContractionSubsequent),
    ("StartOfContrastAgentAdministration", StartOfContrastAgentAdministration),
    ("StartOfDrugDoseAdministration", StartOfDrugDoseAdministration),
    ("StartOfExpiration", StartOfExpiration),
    ("StartOfInspiration", StartOfInspiration),
    ("StartOfThermalCO", StartOfThermalCO),
    ("StartOfXRayIrradiation", StartOfXRayIrradiation),
    ("StartProcedureAction", StartProcedureAction),
    ("StartingFlowRateOfAdministration", StartingFlowRateOfAdministration),
    ("Statement", Statement),
    ("StatementModifier", StatementModifier),
    ("StaticBeam", StaticBeam),
    ("StationAETitle", StationAETitle),
    ("StationaryAcquisition_113611", StationaryAcquisition_113611),
    ("StationaryAcquisition_113806", StationaryAcquisition_113806),
    ("StenoticFlowReserve", StenoticFlowReserve),
    ("StentComposition", StentComposition),
    ("StentDiameterRatio", StentDiameterRatio),
    ("StentDisintegration", StentDisintegration),
    ("StentExpansionIndex", StentExpansionIndex),
    ("StentFracture", StentFracture),
    ("StentGap", StentGap),
    ("StentReExpanded", StentReExpanded),
    ("StentSymmetryIndex", StentSymmetryIndex),
    ("StentVolumeObstruction", StentVolumeObstruction),
    ("StentedRegion", StentedRegion),
    ("StepAndShootBeam", StepAndShootBeam),
    ("SteppingAcquisition", SteppingAcquisition),
    ("StereometricRelationship", StereometricRelationship),
    ("StereotacticCoordinates", StereotacticCoordinates),
    ("StereotacticSetupMethod", StereotacticSetupMethod),
    ("StimulationAtRate1Interval", StimulationAtRate1Interval),
    ("StimulationAtRate2Interval", StimulationAtRate2Interval),
    ("StimulationAtRate3Interval", StimulationAtRate3Interval),
    ("StimulationAtRate4Interval", StimulationAtRate4Interval),
    ("StimulationMode", StimulationMode),
    ("StimulusSamplePosition", StimulusSamplePosition),
    ("StimulusTimeOffset", StimulusTimeOffset),
    ("Stitching", Stitching),
    ("StorageDetector", StorageDetector),
    ("Strain", Strain),
    ("StrainDescription", StrainDescription),
    ("Streamline", Streamline),
    ("StressProtocol", StressProtocol),
    ("StressTechnetiumPerRestTechnetium1DayProcedure", StressTechnetiumPerRestTechnetium1DayProcedure),
    ("StressTestScore", StressTestScore),
    ("StretchedExponentialDiffusionModel", StretchedExponentialDiffusionModel),
    ("StripFilter", StripFilter),
    ("Stripe", Stripe),
    ("Stripes", Stripes),
    ("StronglyPositive", StronglyPositive),
    ("StronglyPositiveSTElevation", StronglyPositiveSTElevation),
    ("StructuralImageForImageProcessing", StructuralImageForImageProcessing),
    ("StructureOfTheFindingSite", StructureOfTheFindingSite),
    ("StructuredReportDocument", StructuredReportDocument),
    ("StudentsTTest", StudentsTTest),
    ("StudiesImported", StudiesImported),
    ("Study", Study),
    ("StudyDate", StudyDate),
    ("StudyInstanceUID", StudyInstanceUID),
    ("StudyPerformedDidNotMatchRequest", StudyPerformedDidNotMatchRequest),
    ("StudyTime", StudyTime),
    ("SubjectAge", SubjectAge),
    ("SubjectBirthDate", SubjectBirthDate),
    ("SubjectBreed", SubjectBreed),
    ("SubjectClass", SubjectClass),
    ("SubjectID", SubjectID),
    ("SubjectName", SubjectName),
    ("SubjectSex", SubjectSex),
    ("SubjectSexParametersForClinicalUseCategory", SubjectSexParametersForClinicalUseCategory),
    ("SubjectSexParametersForClinicalUseCategoryComment", SubjectSexParametersForClinicalUseCategoryComment),
    ("SubjectSexParametersForClinicalUseCategoryReference", SubjectSexParametersForClinicalUseCategoryReference),
    ("SubjectSpecies", SubjectSpecies),
    ("SubjectTimePointIdentifier", SubjectTimePointIdentifier),
    ("SubjectUID", SubjectUID),
    ("SubjectiveRefraction", SubjectiveRefraction),
    ("SuboptimalContrastTiming", SuboptimalContrastTiming),
    ("SuboptimalPatientPreparation", SuboptimalPatientPreparation),
    ("Subpleural", Subpleural),
    ("SubpleuralLine", SubpleuralLine),
    ("SubscapularFossa", SubscapularFossa),
    ("SubstanceUseHistory", SubstanceUseHistory),
    ("SubtractionImageMissing", SubtractionImageMissing),
    ("Succeeded", Succeeded),
    ("SuccessfulAnalyses", SuccessfulAnalyses),
    ("SuccessfulDetections", SuccessfulDetections),
    ("SuggestiveOfMalignancyTakeAppropriateAction", SuggestiveOfMalignancyTakeAppropriateAction),
    ("SumAverageOfGLCM", SumAverageOfGLCM),
    ("SumEntropyOfGLCM", SumEntropyOfGLCM),
    ("SumOfSegmentedVoxelMethodForVolume", SumOfSegmentedVoxelMethodForVolume),
    ("SumVarianceOfGLCM", SumVarianceOfGLCM),
    ("Summary", Summary),
    ("SummaryOfAnalyses", SummaryOfAnalyses),
    ("SummaryOfDetections", SummaryOfDetections),
    ("SummationProjection", SummationProjection),
    ("SuperficialRetinaStructuralReflectanceMap", SuperficialRetinaStructuralReflectanceMap),
    ("SuperficialRetinaVasculatureFlow", SuperficialRetinaVasculatureFlow),
    ("SuperficialVascularPlexusFlow", SuperficialVascularPlexusFlow),
    ("SuperficialVascularPlexusMap", SuperficialVascularPlexusMap),
    ("SuperiorLongitudinalFasciculusI", SuperiorLongitudinalFasciculusI),
    ("SuperiorLongitudinalFasciculusII", SuperiorLongitudinalFasciculusII),
    ("SuperiorLongitudinalFasciculusIII", SuperiorLongitudinalFasciculusIII),
    ("SuperolateralToInferomedial", SuperolateralToInferomedial),
    ("SuperomedialToInferolateral", SuperomedialToInferolateral),
    ("SupplementaryData", SupplementaryData),
    ("SupplementaryDataForIntervention", SupplementaryDataForIntervention),
    ("SupportingInformation", SupportingInformation),
    ("SurfaceBetweenInnerAndOuterSegmentsOfThePhotoreceptors", SurfaceBetweenInnerAndOuterSegmentsOfThePhotoreceptors),
    ("SurfaceContourMapping", SurfaceContourMapping),
    ("SurfaceOfTheCenterOfTheRPE", SurfaceOfTheCenterOfTheRPE),
    ("SurfaceOfTheChoroidScleraInterface", SurfaceOfTheChoroidScleraInterface),
    ("SurfaceOfTheInterdigitationZoneBetweenRetinaAndRPE", SurfaceOfTheInterdigitationZoneBetweenRetinaAndRPE),
    ("SurfaceRendering", SurfaceRendering),
    ("Surgeon", Surgeon),
    ("SurgeonFactor", SurgeonFactor),
    ("SurgicalConsult", SurgicalConsult),
    ("Survey", Survey),
    ("SuspendProcedureAction", SuspendProcedureAction),
    ("SynchronizationFrameOfReferenceUID", SynchronizationFrameOfReferenceUID),
    ("SynchronizedRoboticTreatment", SynchronizedRoboticTreatment),
    ("SynthesizingEquipment", SynthesizingEquipment),
    ("SyringeAttached", SyringeAttached),
    ("SyringeDetached", SyringeDetached),
    ("SystemCalculated", SystemCalculated),
    ("SystemicFlow", SystemicFlow),
    ("SystolicBloodVelocityMean", SystolicBloodVelocityMean),
    ("SystolicBloodVelocityPeak", SystolicBloodVelocityPeak),
    ("SystolicPeakPressure", SystolicPeakPressure),
    ("SystolicPressureAverage", SystolicPressureAverage),
    ("T1", T1),
    ("T1ByFixedValue", T1ByFixedValue),
    ("T1ByInversionRecovery", T1ByInversionRecovery),
    ("T1ByMultipleFlipAngles", T1ByMultipleFlipAngles),
    ("T1UsedForCalculation", T1UsedForCalculation),
    ("T1WeightedDynamicContrastEnhancedMRSignalIntensity", T1WeightedDynamicContrastEnhancedMRSignalIntensity),
    ("T1WeightedMRSignalIntensity", T1WeightedMRSignalIntensity),
    ("T1Worst", T1Worst),
    ("T2Secondary", T2Secondary),
    ("T2WeightedDynamicContrastEnhancedMRSignalIntensity_110817", T2WeightedDynamicContrastEnhancedMRSignalIntensity_110817),
    ("T2WeightedDynamicContrastEnhancedMRSignalIntensity_110818", T2WeightedDynamicContrastEnhancedMRSignalIntensity_110818),
    ("T2WeightedMRSignalIntensity_110805", T2WeightedMRSignalIntensity_110805),
    ("T2WeightedMRSignalIntensity_110806", T2WeightedMRSignalIntensity_110806),
    ("T2_113064", T2_113064),
    ("T2_113065", T2_113065),
    ("T3Secondary", T3Secondary),
    ("T4Secondary", T4Secondary),
    ("T807F18", T807F18),
    ("TBISetupMethod", TBISetupMethod),
    ("TEND", TEND),
    ("TG18ADPattern", TG18ADPattern),
    ("TG18AFCPattern", TG18AFCPattern),
    ("TG18BRPattern", TG18BRPattern),
    ("TG18CHImage", TG18CHImage),
    ("TG18CTPattern", TG18CTPattern),
    ("TG18CXPattern", TG18CXPattern),
    ("TG18GA03Pattern", TG18GA03Pattern),
    ("TG18GA05Pattern", TG18GA05Pattern),
    ("TG18GA08Pattern", TG18GA08Pattern),
    ("TG18GA10Pattern", TG18GA10Pattern),
    ("TG18GA15Pattern", TG18GA15Pattern),
    ("TG18GA20Pattern", TG18GA20Pattern),
    ("TG18GA25Pattern", TG18GA25Pattern),
    ("TG18GA30Pattern", TG18GA30Pattern),
    ("TG18GQBPattern", TG18GQBPattern),
    ("TG18GQNPattern", TG18GQNPattern),
    ("TG18GQPattern", TG18GQPattern),
    ("TG18GVNPattern", TG18GVNPattern),
    ("TG18GVPattern", TG18GVPattern),
    ("TG18KNImage", TG18KNImage),
    ("TG18LN1201Pattern", TG18LN1201Pattern),
    ("TG18LN1202Pattern", TG18LN1202Pattern),
    ("TG18LN1203Pattern", TG18LN1203Pattern),
    ("TG18LN1204Pattern", TG18LN1204Pattern),
    ("TG18LN1205Pattern", TG18LN1205Pattern),
    ("TG18LN1206Pattern", TG18LN1206Pattern),
    ("TG18LN1207Pattern", TG18LN1207Pattern),
    ("TG18LN1208Pattern", TG18LN1208Pattern),
    ("TG18LN1209Pattern", TG18LN1209Pattern),
    ("TG18LN1210Pattern", TG18LN1210Pattern),
    ("TG18LN1211Pattern", TG18LN1211Pattern),
    ("TG18LN1212Pattern", TG18LN1212Pattern),
    ("TG18LN1213Pattern", TG18LN1213Pattern),
    ("TG18LN1214Pattern", TG18LN1214Pattern),
    ("TG18LN1215Pattern", TG18LN1215Pattern),
    ("TG18LN1216Pattern", TG18LN1216Pattern),
    ("TG18LN1217Pattern", TG18LN1217Pattern),
    ("TG18LN1218Pattern", TG18LN1218Pattern),
    ("TG18LN801Pattern", TG18LN801Pattern),
    ("TG18LN802Pattern", TG18LN802Pattern),
    ("TG18LN803Pattern", TG18LN803Pattern),
    ("TG18LN804Pattern", TG18LN804Pattern),
    ("TG18LN805Pattern", TG18LN805Pattern),
    ("TG18LN806Pattern", TG18LN806Pattern),
    ("TG18LN807Pattern", TG18LN807Pattern),
    ("TG18LN808Pattern", TG18LN808Pattern),
    ("TG18LN809Pattern", TG18LN809Pattern),
    ("TG18LN810Pattern", TG18LN810Pattern),
    ("TG18LN811Pattern", TG18LN811Pattern),
    ("TG18LN812Pattern", TG18LN812Pattern),
    ("TG18LN813Pattern", TG18LN813Pattern),
    ("TG18LN814Pattern", TG18LN814Pattern),
    ("TG18LN815Pattern", TG18LN815Pattern),
    ("TG18LN816Pattern", TG18LN816Pattern),
    ("TG18LN817Pattern", TG18LN817Pattern),
    ("TG18LN818Pattern", TG18LN818Pattern),
    ("TG18LPH10Pattern", TG18LPH10Pattern),
    ("TG18LPH50Pattern", TG18LPH50Pattern),
    ("TG18LPH89Pattern", TG18LPH89Pattern),
    ("TG18LPV10Pattern", TG18LPV10Pattern),
    ("TG18LPV50Pattern", TG18LPV50Pattern),
    ("TG18LPV89Pattern", TG18LPV89Pattern),
    ("TG18MM1Image", TG18MM1Image),
    ("TG18MM2Image", TG18MM2Image),
    ("TG18MPPattern", TG18MPPattern),
    ("TG18NS10Pattern", TG18NS10Pattern),
    ("TG18NS50Pattern", TG18NS50Pattern),
    ("TG18NS89Pattern", TG18NS89Pattern),
    ("TG18PQCPattern", TG18PQCPattern),
    ("TG18PXPattern", TG18PXPattern),
    ("TG18QCPattern", TG18QCPattern),
    ("TG18RH10Pattern", TG18RH10Pattern),
    ("TG18RH50Pattern", TG18RH50Pattern),
    ("TG18RH89Pattern", TG18RH89Pattern),
    ("TG18RV10Pattern", TG18RV10Pattern),
    ("TG18RV50Pattern", TG18RV50Pattern),
    ("TG18RV89Pattern", TG18RV89Pattern),
    ("TG18UN10Pattern", TG18UN10Pattern),
    ("TG18UN80Pattern", TG18UN80Pattern),
    ("TG18UNL10Pattern", TG18UNL10Pattern),
    ("TG18UNL80Pattern", TG18UNL80Pattern),
    ("THK5351F18", THK5351F18),
    ("TMRAndOARRatios", TMRAndOARRatios),
    ("TRC10589Zr", TRC10589Zr),
    ("TSPyramid", TSPyramid),
    ("TWave", TWave),
    ("Table", Table),
    ("TableCore", TableCore),
    ("TableCradleTiltAngle", TableCradleTiltAngle),
    ("TableCradleTiltEndAngle", TableCradleTiltEndAngle),
    ("TableHeadTiltAngle", TableHeadTiltAngle),
    ("TableHeadTiltEndAngle", TableHeadTiltEndAngle),
    ("TableHeightEndPosition", TableHeightEndPosition),
    ("TableHeightPosition", TableHeightPosition),
    ("TableHorizontalRotationAngle", TableHorizontalRotationAngle),
    ("TableHorizontalRotationEndAngle", TableHorizontalRotationEndAngle),
    ("TableLateralEndPosition", TableLateralEndPosition),
    ("TableLateralPosition", TableLateralPosition),
    ("TableLongitudinalEndPosition", TableLongitudinalEndPosition),
    ("TableLongitudinalPosition", TableLongitudinalPosition),
    ("TableMount", TableMount),
    ("TableOfValues", TableOfValues),
    ("TableOfValuesCitation", TableOfValuesCitation),
    ("TableOuterLiner", TableOuterLiner),
    ("TablePad", TablePad),
    ("TableXEndPositionToIsocenter", TableXEndPositionToIsocenter),
    ("TableXPositionToIsocenter", TableXPositionToIsocenter),
    ("TableYEndPositionToIsocenter", TableYEndPositionToIsocenter),
    ("TableYPositionToIsocenter", TableYPositionToIsocenter),
    ("TableZEndPositionToIsocenter", TableZEndPositionToIsocenter),
    ("TableZPositionToIsocenter", TableZPositionToIsocenter),
    ("TabularDataAlgorithm", TabularDataAlgorithm),
    ("TapTestBegin", TapTestBegin),
    ("TapTestEnd", TapTestEnd),
    ("TargestarB", TargestarB),
    ("TargestarP", TargestarP),
    ("Target", Target),
    ("TargetCompletelyContainedInTheSpecimen", TargetCompletelyContainedInTheSpecimen),
    ("TargetContentItemsAreRelatedContraLaterally", TargetContentItemsAreRelatedContraLaterally),
    ("TargetContentItemsAreRelatedSpatially", TargetContentItemsAreRelatedSpatially),
    ("TargetContentItemsAreRelatedTemporally", TargetContentItemsAreRelatedTemporally),
    ("TargetExposureIndex", TargetExposureIndex),
    ("TargetLesionAtBaseline", TargetLesionAtBaseline),
    ("TargetLesionCompleteResponse", TargetLesionCompleteResponse),
    ("TargetLesionPartialResponse", TargetLesionPartialResponse),
    ("TargetLesionProgressiveDisease", TargetLesionProgressiveDisease),
    ("TargetLesionStableDisease", TargetLesionStableDisease),
    ("TargetNotInTheSpecimen", TargetNotInTheSpecimen),
    ("TargetPartiallyObtainedInTheSpecimen", TargetPartiallyObtainedInTheSpecimen),
    ("TargetRegion", TargetRegion),
    ("Targeted", Targeted),
    ("Tau", Tau),
    ("TauM", TauM),
    ("TaxonomicRankOfOrigin", TaxonomicRankOfOrigin),
    ("TeachingFileCategory", TeachingFileCategory),
    ("TechNote", TechNote),
    ("TechnicalFactorsMissing", TechnicalFactorsMissing),
    ("Technologist", Technologist),
    ("Teichholz", Teichholz),
    ("TeletherapyIsotope", TeletherapyIsotope),
    ("TemperatureEncoded", TemperatureEncoded),
    ("TemporalDerivativeExceedsThreshold", TemporalDerivativeExceedsThreshold),
    ("TemporalDerivativeThreshold", TemporalDerivativeThreshold),
    ("TemporalPredecessor", TemporalPredecessor),
    ("TemporalSuccessor", TemporalSuccessor),
    ("TemporallyRelatedFramesExtractedFromTheSetOfVolumes", TemporallyRelatedFramesExtractedFromTheSetOfVolumes),
    ("TerminatedByScanner", TerminatedByScanner),
    ("TerminatedDueToAirDetected", TerminatedDueToAirDetected),
    ("TerminatedDueToConsumableRemoval", TerminatedDueToConsumableRemoval),
    ("TerminatedDueToCriticalBatteryLevel", TerminatedDueToCriticalBatteryLevel),
    ("TerminatedDueToExcessiveDurationPause", TerminatedDueToExcessiveDurationPause),
    ("TerminatedDueToFlowRateAboveTerminationLimit", TerminatedDueToFlowRateAboveTerminationLimit),
    ("TerminatedDueToInjectorCommunicationLoss", TerminatedDueToInjectorCommunicationLoss),
    ("TerminatedDueToPressureAboveTerminationLimit", TerminatedDueToPressureAboveTerminationLimit),
    ("TerminatedDueToRequestFromOperator", TerminatedDueToRequestFromOperator),
    ("TerminatedDueToUnspecifiedInjectorFailure", TerminatedDueToUnspecifiedInjectorFailure),
    ("TerminatedHoldDueToTimeout", TerminatedHoldDueToTimeout),
    ("TerritoryRegionSeverity", TerritoryRegionSeverity),
    ("TextureDescriptor", TextureDescriptor),
    ("TextureMap", TextureMap),
    ("ThermalImager", ThermalImager),
    ("Thermography", Thermography),
    ("ThickeningAnalysis", ThickeningAnalysis),
    ("ThicknessDeviationCategoryFromNormativeData", ThicknessDeviationCategoryFromNormativeData),
    ("ThicknessDeviationFromNormativeData", ThicknessDeviationFromNormativeData),
    ("ThinningMyocardium", ThinningMyocardium),
    ("ThoracicImagingSpecialty", ThoracicImagingSpecialty),
    ("ThoracicRadiology", ThoracicRadiology),
    ("ThoracicRespiratoryEffort", ThoracicRespiratoryEffort),
    ("ThoracicRespiratoryInductance", ThoracicRespiratoryInductance),
    ("ThoracicRespiratoryPVDF", ThoracicRespiratoryPVDF),
    ("ThoraxDiameterSagittal", ThoraxDiameterSagittal),
    ("ThreeDimensionalMethod", ThreeDimensionalMethod),
    ("ThreeVesselAndTracheaView", ThreeVesselAndTracheaView),
    ("ThreeVesselView", ThreeVesselView),
    ("ThresholdAttenuationCoefficient", ThresholdAttenuationCoefficient),
    ("ThresholdRelated", ThresholdRelated),
    ("ThresholdValue", ThresholdValue),
    ("ThroughTransducerIllumination", ThroughTransducerIllumination),
    ("Time", Time),
    ("TimeAfterStartOfRadiation", TimeAfterStartOfRadiation),
    ("TimeAfterTheStartOfInjection", TimeAfterTheStartOfInjection),
    ("TimeCourseOfSignal", TimeCourseOfSignal),
    ("TimeDomain", TimeDomain),
    ("TimeIntervalSinceDetectionOfContrastBolus", TimeIntervalSinceDetectionOfContrastBolus),
    ("TimeIntervalSinceInjectionOfContrastMedia", TimeIntervalSinceInjectionOfContrastMedia),
    ("TimeOfFlight", TimeOfFlight),
    ("TimeOfFlightAngioMRSignalIntensity", TimeOfFlightAngioMRSignalIntensity),
    ("TimeOfLeadingHalfPeakConcentration", TimeOfLeadingHalfPeakConcentration),
    ("TimeOfPeakConcentration", TimeOfPeakConcentration),
    ("TimePointOrder", TimePointOrder),
    ("TimePointType", TimePointType),
    ("TimeRelativeToRWavePeak", TimeRelativeToRWavePeak),
    ("TimeSinceStartOfStage", TimeSinceStartOfStage),
    ("TimeToPeak", TimeToPeak),
    ("TimeToPeakLongitudinalLagrangianStrain", TimeToPeakLongitudinalLagrangianStrain),
    ("TimeframeUncertain", TimeframeUncertain),
    ("TimerInterlock", TimerInterlock),
    ("TissueAirRatio", TissueAirRatio),
    ("TissueFraction", TissueFraction),
    ("TissueMarking", TissueMarking),
    ("TissueNotOfInterest", TissueNotOfInterest),
    ("TissueOfOrigin", TissueOfOrigin),
    ("TissueSelectionMethod", TissueSelectionMethod),
    ("TissueVelocity", TissueVelocity),
    ("TitaniumAlloyTitaniumMolybdenum15Zirconium5Aluminium3", TitaniumAlloyTitaniumMolybdenum15Zirconium5Aluminium3),
    ("Title21CFRSection900SubpartB", Title21CFRSection900SubpartB),
    ("Tmax", Tmax),
    ("TooSmall", TooSmall),
    ("ToolFabrication", ToolFabrication),
    ("TopZLocationOfReconstructableVolume", TopZLocationOfReconstructableVolume),
    ("TopZLocationOfScanningLength", TopZLocationOfScanningLength),
    ("TopographicBeam", TopographicBeam),
    ("TotalAcquisitionTime", TotalAcquisitionTime),
    ("TotalAntralFollicleCount", TotalAntralFollicleCount),
    ("TotalBodyIrradiation", TotalBodyIrradiation),
    ("TotalCholesterolScoreSheetForMen", TotalCholesterolScoreSheetForMen),
    ("TotalCholesterolScoreSheetForWomen", TotalCholesterolScoreSheetForWomen),
    ("TotalCorneaPowerMeasurementMethod", TotalCorneaPowerMeasurementMethod),
    ("TotalDurationInHousing", TotalDurationInHousing),
    ("TotalDurationOfLightDarkCycle", TotalDurationOfLightDarkCycle),
    ("TotalFluoroTime", TotalFluoroTime),
    ("TotalInternalReflectionFluorescence", TotalInternalReflectionFluorescence),
    ("TotalKeepVeinOpenVolumeAdministered", TotalKeepVeinOpenVolumeAdministered),
    ("TotalLesionGlycolysis", TotalLesionGlycolysis),
    ("TotalLesionProliferation", TotalLesionProliferation),
    ("TotalNumberOfIrradiationEvents", TotalNumberOfIrradiationEvents),
    ("TotalNumberOfManuallyTriggeredInjections", TotalNumberOfManuallyTriggeredInjections),
    ("TotalNumberOfRadiographicFrames", TotalNumberOfRadiographicFrames),
    ("TotalPhaseVolumeAdministered", TotalPhaseVolumeAdministered),
    ("TotalPlaqueVolume", TotalPlaqueVolume),
    ("TotalPulmonaryResistance", TotalPulmonaryResistance),
    ("TotalRetinalThicknessILMToBM", TotalRetinalThicknessILMToBM),
    ("TotalRetinalThicknessILMToISOS", TotalRetinalThicknessILMToISOS),
    ("TotalRetinalThicknessILMToRPE", TotalRetinalThicknessILMToRPE),
    ("TotalSkinIrradiation", TotalSkinIrradiation),
    ("TotalStepVolumeAdministered", TotalStepVolumeAdministered),
    ("TotalVascularResistance", TotalVascularResistance),
    ("Trace", Trace),
    ("TracerConcentration", TracerConcentration),
    ("TrackingIdentifier", TrackingIdentifier),
    ("TrackingUniqueIdentifier", TrackingUniqueIdentifier),
    ("TramlineShadow", TramlineShadow),
    ("Transcription_110006", Transcription_110006),
    ("Transcription_110012", Transcription_110012),
    ("TransesophagealTransducer", TransesophagealTransducer),
    ("TransformationMatrix", TransformationMatrix),
    ("TransitTimeTestInjection", TransitTimeTestInjection),
    ("TransmissionIllumination", TransmissionIllumination),
    ("TransmitFrequency", TransmitFrequency),
    ("TransponderMarker", TransponderMarker),
    ("TransseptalPunctureHeight", TransseptalPunctureHeight),
    ("TransthalamicCoronalView", TransthalamicCoronalView),
    ("Trastuzumab89Zr", Trastuzumab89Zr),
    ("TreadmillGradient", TreadmillGradient),
    ("TreadmillSpeed", TreadmillSpeed),
    ("TreatedVolume", TreatedVolume),
    ("TreatmentDeliveryType", TreatmentDeliveryType),
    ("TreatmentExceedsPatientTolerance", TreatmentExceedsPatientTolerance),
    ("TreatmentMachineIsocenter", TreatmentMachineIsocenter),
    ("TreatmentPlanningNote", TreatmentPlanningNote),
    ("TreatmentTerminated", TreatmentTerminated),
    ("TreatmentUnnecessary", TreatmentUnnecessary),
    ("TreeInBudSign", TreeInBudSign),
    ("Trend", Trend),
    ("TricuspidAnnulusArea", TricuspidAnnulusArea),
    ("TricuspidAnnulusAreaDiastolicSystolicRatio", TricuspidAnnulusAreaDiastolicSystolicRatio),
    ("TricuspidAnnulusPerimeter", TricuspidAnnulusPerimeter),
    ("TricuspidValveAnnulusDiameter", TricuspidValveAnnulusDiameter),
    ("TricuspidValveCoaptationLength", TricuspidValveCoaptationLength),
    ("TricuspidValveMajorAxisDiastole", TricuspidValveMajorAxisDiastole),
    ("TricuspidValveMinorAxis", TricuspidValveMinorAxis),
    ("TricuspidValveSphericityIndex", TricuspidValveSphericityIndex),
    ("TricuspidValveTentingHeight", TricuspidValveTentingHeight),
    ("TricuspidValveTentingVolume", TricuspidValveTentingVolume),
    ("TrigeminalNodeSet", TrigeminalNodeSet),
    ("TrueLumen", TrueLumen),
    ("TruncatedGaussianDiffusionModel", TruncatedGaussianDiffusionModel),
    ("TrustRegion", TrustRegion),
    ("TubularShadow", TubularShadow),
    ("TumorBoardInputUsed", TumorBoardInputUsed),
    ("TumorGraft", TumorGraft),
    ("TumorRegistryInputUsed", TumorRegistryInputUsed),
    ("TurbulenceResistance", TurbulenceResistance),
    ("TurntableScanMethod", TurntableScanMethod),
    ("TwoChamber", TwoChamber),
    ("TwoCompartmentExchangeModel", TwoCompartmentExchangeModel),
    ("TwoDimensionalMethod", TwoDimensionalMethod),
    ("TypeOfContent", TypeOfContent),
    ("TypeOfMyocardialInfarction", TypeOfMyocardialInfarction),
    ("UDI", UDI),
    ("URI", URI),
    ("US3DCAMModel", US3DCAMModel),
    ("USBDiskEmulation", USBDiskEmulation),
    ("Ublituximab89Zr", Ublituximab89Zr),
    ("Ultrasound", Ultrasound),
    ("UltrasoundAttenuation", UltrasoundAttenuation),
    ("UltrasoundAttenuationCoefficient", UltrasoundAttenuationCoefficient),
    ("UltrasoundAttenuationImaging", UltrasoundAttenuationImaging),
    ("UltrasoundBoneDensitometry", UltrasoundBoneDensitometry),
    ("UltrasoundContact", UltrasoundContact),
    ("UltrasoundImagingSubjectMatter", UltrasoundImagingSubjectMatter),
    ("UltrasoundImmersion", UltrasoundImmersion),
    ("Ultrasound_111137", Ultrasound_111137),
    ("UmbilicocerebralRatio", UmbilicocerebralRatio),
    ("UnavailabilityOfABeamModifier", UnavailabilityOfABeamModifier),
    ("Unavailable", Unavailable),
    ("UnclassifiedCombination", UnclassifiedCombination),
    ("UnclassifiedVolume", UnclassifiedVolume),
    ("UncompressedPredecessor", UncompressedPredecessor),
    ("UnderExposed", UnderExposed),
    ("Underflow", Underflow),
    ("UndeterminedSex", UndeterminedSex),
    ("UndilutedDoseAdministered", UndilutedDoseAdministered),
    ("Unheated", Unheated),
    ("UniformSpeedOfSoundCorrection", UniformSpeedOfSoundCorrection),
    ("Uninvolved", Uninvolved),
    ("UniqueDeviceIdentifiers", UniqueDeviceIdentifiers),
    ("UnitSerialIdentifier", UnitSerialIdentifier),
    ("UnitedAllBreedRegistry", UnitedAllBreedRegistry),
    ("UnitedKennelClub", UnitedKennelClub),
    ("UniversalKennelClubInternational", UniversalKennelClubInternational),
    ("UnknownFailure", UnknownFailure),
    ("UnknownSex", UnknownSex),
    ("UnmountedDetector", UnmountedDetector),
    ("UnscannedTissue", UnscannedTissue),
    ("UnsharpMask", UnsharpMask),
    ("Unspecified", Unspecified),
    ("UnspecifiedGynecologicalHormone", UnspecifiedGynecologicalHormone),
    ("UnspecifiedMethodOfCalculation", UnspecifiedMethodOfCalculation),
    ("UntrackableMeasurement", UntrackableMeasurement),
    ("UnusableQualityRendersImageUnusable", UnusableQualityRendersImageUnusable),
    ("UnusableTissue", UnusableTissue),
    ("Unused", Unused),
    ("UnwantedSubtractionImages", UnwantedSubtractionImages),
    ("UprightStandMount", UprightStandMount),
    ("UsableDoesNotMeetTheQualityControlStandard", UsableDoesNotMeetTheQualityControlStandard),
    ("UsableMeetsTheQualityControlStandard", UsableMeetsTheQualityControlStandard),
    ("UsableTissue", UsableTissue),
    ("UseOfRestrictedFunction", UseOfRestrictedFunction),
    ("UsedSubstanceType", UsedSubstanceType),
    ("UserAuthentication", UserAuthentication),
    ("UserChosenValue", UserChosenValue),
    ("UserDefinedAIFROI", UserDefinedAIFROI),
    ("UserSecurityAttributesChanged", UserSecurityAttributesChanged),
    ("UserSelectedFiducial", UserSelectedFiducial),
    ("UserSelectedMethod", UserSelectedMethod),
    ("UserSelectedVolumeFlow", UserSelectedVolumeFlow),
    ("UserSelectedVolumeStructureMap", UserSelectedVolumeStructureMap),
    ("UterineMalformations", UterineMalformations),
    ("UtricleCyst", UtricleCyst),
    ("VMAT", VMAT),
    ("VMaxMyocardial", VMaxMyocardial),
    ("VO2Equals152BSA", VO2Equals152BSA),
    ("VO2Equals175BSA", VO2Equals175BSA),
    ("VO2Equals176BSA", VO2Equals176BSA),
    ("VO2EqualsVeSTPD10FIO2FE02", VO2EqualsVeSTPD10FIO2FE02),
    ("VO2femaleEqualsBSA13811704LogeAgePlus0378HRf", VO2femaleEqualsBSA13811704LogeAgePlus0378HRf),
    ("VO2maleEqualsBSA13811149LogeAgePlus0378HRf", VO2maleEqualsBSA13811149LogeAgePlus0378HRf),
    ("VVulnerablePlaque", VVulnerablePlaque),
    ("VWave", VWave),
    ("VWaveOfNextBeat", VWaveOfNextBeat),
    ("VWavePeakPressure", VWavePeakPressure),
    ("VWavePressureAverage", VWavePressureAverage),
    ("VacuumAssisted", VacuumAssisted),
    ("VacuumMold", VacuumMold),
    ("VaginalCylinder", VaginalCylinder),
    ("ValueBasedImage", ValueBasedImage),
    ("ValueIndeterminate", ValueIndeterminate),
    ("ValueOutOfRange", ValueOutOfRange),
    ("ValueTiming", ValueTiming),
    ("ValueUnknown", ValueUnknown),
    ("ValveClose", ValveClose),
    ("ValveOpen", ValveOpen),
    ("ValvularIntervention", ValvularIntervention),
    ("VariableCircularCollimator", VariableCircularCollimator),
    ("VascularAndInterventionalImagingSubjectMatter", VascularAndInterventionalImagingSubjectMatter),
    ("VascularFunctionalAnalysis", VascularFunctionalAnalysis),
    ("VascularImagingSpecialty", VascularImagingSpecialty),
    ("VascularIntervention", VascularIntervention),
    ("VascularMorphologicalAnalysis", VascularMorphologicalAnalysis),
    ("VascularUltrasoundProcedureReport", VascularUltrasoundProcedureReport),
    ("VascularVolumeMeasurementLength", VascularVolumeMeasurementLength),
    ("Vascularity", Vascularity),
    ("VascularityNotAssessed", VascularityNotAssessed),
    ("VascularityNotPresent", VascularityNotPresent),
    ("VascularityPresentImmediatelyAdjacentToLesion", VascularityPresentImmediatelyAdjacentToLesion),
    ("VascularityPresentInLesion", VascularityPresentInLesion),
    ("Vasoconstriction", Vasoconstriction),
    ("Vasodilation", Vasodilation),
    ("Ve", Ve),
    ("Vector", Vector),
    ("VeilingGlareEvaluation", VeilingGlareEvaluation),
    ("VelocityEncoded", VelocityEncoded),
    ("VelocityEncodingMaximumValue", VelocityEncodingMaximumValue),
    ("VelocityEncodingMinimumValue", VelocityEncodingMinimumValue),
    ("VenaContractaWidth", VenaContractaWidth),
    ("VenousDopplerScore", VenousDopplerScore),
    ("VentralDiencephalon", VentralDiencephalon),
    ("VentricularAnalysis", VentricularAnalysis),
    ("VentricularEndDiastolicPressure", VentricularEndDiastolicPressure),
    ("VentricularPressureMeasurements", VentricularPressureMeasurements),
    ("VentricularSystolicBloodPressure", VentricularSystolicBloodPressure),
    ("VentriculoatrialTimeInterval", VentriculoatrialTimeInterval),
    ("Verbal", Verbal),
    ("Verifying", Verifying),
    ("VertebralIntervertebralNotch", VertebralIntervertebralNotch),
    ("VerticalPixelSpacing", VerticalPixelSpacing),
    ("VerticalPrismBase", VerticalPrismBase),
    ("VerticalPrismPower", VerticalPrismPower),
    ("VerySmall", VerySmall),
    ("VeryStrongFamilyHistoryOfBreastCancer", VeryStrongFamilyHistoryOfBreastCancer),
    ("VesselBranch", VesselBranch),
    ("VesselLumenCrossSectionalArea", VesselLumenCrossSectionalArea),
    ("VesselMorphology", VesselMorphology),
    ("VideoRecordingOff", VideoRecordingOff),
    ("VideoRecordingOn", VideoRecordingOn),
    ("VideoTapeDigitized", VideoTapeDigitized),
    ("VideoTapeDigitizerEquipment", VideoTapeDigitizerEquipment),
    ("Videofluorography", Videofluorography),
    ("ViewAndLateralityMarkerDoesNotHaveApprovedCodes", ViewAndLateralityMarkerDoesNotHaveApprovedCodes),
    ("ViewAndLateralityMarkerDoesNotHaveBothViewAndLaterality", ViewAndLateralityMarkerDoesNotHaveBothViewAndLaterality),
    ("ViewAndLateralityMarkerIsIncorrect", ViewAndLateralityMarkerIsIncorrect),
    ("ViewAndLateralityMarkerIsMissing", ViewAndLateralityMarkerIsMissing),
    ("ViewAndLateralityMarkerIsNotNearTheAxilla", ViewAndLateralityMarkerIsNotNearTheAxilla),
    ("ViewAndLateralityMarkerIsOffImage", ViewAndLateralityMarkerIsOffImage),
    ("ViewAndLateralityMarkerIsPartiallyObscured", ViewAndLateralityMarkerIsPartiallyObscured),
    ("ViewAndLateralityMarkerOverlapsBreastTissue", ViewAndLateralityMarkerOverlapsBreastTissue),
    ("Vigneswaran2018", Vigneswaran2018),
    ("VirginalHyperplasia", VirginalHyperplasia),
    ("VirtualGrid", VirtualGrid),
    ("ViscosityAt37C", ViscosityAt37C),
    ("VisibleReflectorMarker", VisibleReflectorMarker),
    ("VisualAcuity", VisualAcuity),
    ("VisualAlignment", VisualAlignment),
    ("VisualExplanation", VisualExplanation),
    ("VisualField07TestPattern", VisualField07TestPattern),
    ("VisualField102TestPattern", VisualField102TestPattern),
    ("VisualField1LTTestStrategy", VisualField1LTTestStrategy),
    ("VisualField242TestPattern", VisualField242TestPattern),
    ("VisualField2LTDynamicTestStrategy", VisualField2LTDynamicTestStrategy),
    ("VisualField2LTNormalTestStrategy", VisualField2LTNormalTestStrategy),
    ("VisualField2LTTestStrategy", VisualField2LTTestStrategy),
    ("VisualField302TestPattern", VisualField302TestPattern),
    ("VisualField604TestPattern", VisualField604TestPattern),
    ("VisualFieldCLASSStrategy", VisualFieldCLASSStrategy),
    ("VisualFieldCLIPTestStrategy", VisualFieldCLIPTestStrategy),
    ("VisualFieldCentral40PointTestPattern", VisualFieldCentral40PointTestPattern),
    ("VisualFieldCentral76PointTestPattern", VisualFieldCentral76PointTestPattern),
    ("VisualFieldCentralTestPattern", VisualFieldCentralTestPattern),
    ("VisualFieldDynamicTestStrategy", VisualFieldDynamicTestStrategy),
    ("VisualFieldFastPacTestStrategy", VisualFieldFastPacTestStrategy),
    ("VisualFieldFastThresholdTestStrategy", VisualFieldFastThresholdTestStrategy),
    ("VisualFieldFullField120PointTestPattern", VisualFieldFullField120PointTestPattern),
    ("VisualFieldFullField81PointTestPattern", VisualFieldFullField81PointTestPattern),
    ("VisualFieldFullFromPriorTestStrategy", VisualFieldFullFromPriorTestStrategy),
    ("VisualFieldFullThresholdTestStrategy", VisualFieldFullThresholdTestStrategy),
    ("VisualFieldGATETestStrategy", VisualFieldGATETestStrategy),
    ("VisualFieldGATEiTestStrategy", VisualFieldGATEiTestStrategy),
    ("VisualFieldGTestPattern", VisualFieldGTestPattern),
    ("VisualFieldGlobalDeviationFromNormal", VisualFieldGlobalDeviationFromNormal),
    ("VisualFieldIndex", VisualFieldIndex),
    ("VisualFieldKeyMeasurements", VisualFieldKeyMeasurements),
    ("VisualFieldLVCTestPattern", VisualFieldLVCTestPattern),
    ("VisualFieldLVSTestStrategy", VisualFieldLVSTestStrategy),
    ("VisualFieldLocalizedDeviationFromNormal", VisualFieldLocalizedDeviationFromNormal),
    ("VisualFieldLossDueToDiffuseDefect", VisualFieldLossDueToDiffuseDefect),
    ("VisualFieldLossDueToLocalDefect", VisualFieldLossDueToLocalDefect),
    ("VisualFieldMTestPattern", VisualFieldMTestPattern),
    ("VisualFieldMaculaTestPattern", VisualFieldMaculaTestPattern),
    ("VisualFieldNormalTestStrategy", VisualFieldNormalTestStrategy),
    ("VisualFieldOptimaTestStrategy", VisualFieldOptimaTestStrategy),
    ("VisualFieldPeripheral60PointTestPattern", VisualFieldPeripheral60PointTestPattern),
    ("VisualFieldQuantifyDefectsTestStrategy", VisualFieldQuantifyDefectsTestStrategy),
    ("VisualFieldSITAFastTestStrategy", VisualFieldSITAFastTestStrategy),
    ("VisualFieldSITASWAPTestStrategy", VisualFieldSITASWAPTestStrategy),
    ("VisualFieldSITAStandardTestStrategy", VisualFieldSITAStandardTestStrategy),
    ("VisualFieldTOPTestStrategy", VisualFieldTOPTestStrategy),
    ("VisualFieldThreeZoneTestStrategy", VisualFieldThreeZoneTestStrategy),
    ("VisualFieldTwoZoneTestStrategy", VisualFieldTwoZoneTestStrategy),
    ("VisualFixationQualityDuringAcquisition", VisualFixationQualityDuringAcquisition),
    ("VisualFixationQualityProblem", VisualFixationQualityProblem),
    ("VisualMarkingOfSpecimen", VisualMarkingOfSpecimen),
    ("VitreousStructuralReflectanceMap", VitreousStructuralReflectanceMap),
    ("VitreousVasculatureFlow", VitreousVasculatureFlow),
    ("Voice", Voice),
    ("VoltageMeasurement", VoltageMeasurement),
    ("VoltageMeasurementByBasketCatheter", VoltageMeasurementByBasketCatheter),
    ("VoltageMeasurementByMappingCatheter", VoltageMeasurementByMappingCatheter),
    ("VolumeAdministered", VolumeAdministered),
    ("VolumeBasedMethod", VolumeBasedMethod),
    ("VolumeCorrespondingToSpatiallyRelatedAcquisitionFrames", VolumeCorrespondingToSpatiallyRelatedAcquisitionFrames),
    ("VolumeEditing", VolumeEditing),
    ("VolumeEstimatedFromSingle2DRegion", VolumeEstimatedFromSingle2DRegion),
    ("VolumeEstimatedFromThreeOrMoreNonCoplanar2DRegions", VolumeEstimatedFromThreeOrMoreNonCoplanar2DRegions),
    ("VolumeEstimatedFromTwoNonCoplanar2DRegions", VolumeEstimatedFromTwoNonCoplanar2DRegions),
    ("VolumeMethod", VolumeMethod),
    ("VolumeOccupancyImage", VolumeOccupancyImage),
    ("VolumeOfBoundingThreeDimensionalRegion", VolumeOfBoundingThreeDimensionalRegion),
    ("VolumeOfBreast", VolumeOfBreast),
    ("VolumeOfCircumscribedSphere", VolumeOfCircumscribedSphere),
    ("VolumeOfEllipsoid", VolumeOfEllipsoid),
    ("VolumeOfParenchymalTissue", VolumeOfParenchymalTissue),
    ("VolumeOfSphere", VolumeOfSphere),
    ("VolumeOfUse", VolumeOfUse),
    ("VolumeOfVascularCalcification", VolumeOfVascularCalcification),
    ("VolumeRangeMaximumValue", VolumeRangeMaximumValue),
    ("VolumeRangeMinimumValue", VolumeRangeMinimumValue),
    ("VolumeRatio", VolumeRatio),
    ("VolumeRendering", VolumeRendering),
    ("VolumeScanPattern", VolumeScanPattern),
    ("VolumeScoringMethod", VolumeScoringMethod),
    ("VolumeSurface", VolumeSurface),
    ("VolumeUnadministeredOrDiscarded", VolumeUnadministeredOrDiscarded),
    ("VolumetricDiffusionDxxComponent", VolumetricDiffusionDxxComponent),
    ("VolumetricDiffusionDxyComponent", VolumetricDiffusionDxyComponent),
    ("VolumetricDiffusionDxzComponent", VolumetricDiffusionDxzComponent),
    ("VolumetricDiffusionDyyComponent", VolumetricDiffusionDyyComponent),
    ("VolumetricDiffusionDyzComponent", VolumetricDiffusionDyzComponent),
    ("VolumetricDiffusionDzzComponent", VolumetricDiffusionDzzComponent),
    ("VoxelizedRadiationTransportModel", VoxelizedRadiationTransportModel),
    ("VoxelwiseSelectionOfBValue", VoxelwiseSelectionOfBValue),
    ("Vp", Vp),
    ("VulnerablePlaque", VulnerablePlaque),
    ("WHO", WHO),
    ("WSI20XRGB", WSI20XRGB),
    ("WSI40XRGB", WSI40XRGB),
    ("WallMass", WallMass),
    ("WallStress", WallStress),
    ("WallThickness", WallThickness),
    ("WallThicknessRatioEndSystolicToEndDiastolic", WallThicknessRatioEndSystolicToEndDiastolic),
    ("WallToSeptum", WallToSeptum),
    ("WallVolume", WallVolume),
    ("WashoutDelayedPhaseType3Curve", WashoutDelayedPhaseType3Curve),
    ("WaterEquivalentDiameter", WaterEquivalentDiameter),
    ("WaterEquivalentDiameterAtLongitudinalPositionZ", WaterEquivalentDiameterAtLongitudinalPositionZ),
    ("WaterEquivalentDiameterFromLimitedFOVImages", WaterEquivalentDiameterFromLimitedFOVImages),
    ("WaterEquivalentDiameterFromLocalizer", WaterEquivalentDiameterFromLocalizer),
    ("WaterEquivalentDiameterFromRawData", WaterEquivalentDiameterFromRawData),
    ("WaterEquivalentDiameterIntegratedAcrossScanRange", WaterEquivalentDiameterIntegratedAcrossScanRange),
    ("WaterEquivalentDiameterRepresentativeValue", WaterEquivalentDiameterRepresentativeValue),
    ("WaterFraction", WaterFraction),
    ("WaterPerFatInPhase", WaterPerFatInPhase),
    ("WaterPerFatOutOfPhase", WaterPerFatOutOfPhase),
    ("WaveformAcquired", WaveformAcquired),
    ("WaveformAnnotationGroup", WaveformAnnotationGroup),
    ("WaveformAnnotationGroupLabel", WaveformAnnotationGroupLabel),
    ("WaveformAnnotationGroupNumber", WaveformAnnotationGroupNumber),
    ("WaveformAnnotationModifier", WaveformAnnotationModifier),
    ("WaveformAnnotations", WaveformAnnotations),
    ("WaveformLibrary", WaveformLibrary),
    ("WaveformLibraryEntryMultiplexGroupDescriptors", WaveformLibraryEntryMultiplexGroupDescriptors),
    ("WaveformLibraryGroup", WaveformLibraryGroup),
    ("WeakFamilyHistoryOfBreastCancer", WeakFamilyHistoryOfBreastCancer),
    ("WedgeFilter", WedgeFilter),
    ("Wedging", Wedging),
    ("WellDemarcated", WellDemarcated),
    ("WhiteMatterT1Hypointensity", WhiteMatterT1Hypointensity),
    ("WhiteMatterT2Hyperintensity", WhiteMatterT2Hyperintensity),
    ("WhitePattern", WhitePattern),
    ("WholeBodyPod", WholeBodyPod),
    ("WholeEyeStructuralReflectanceMap", WholeEyeStructuralReflectanceMap),
    ("WholeEyeVasculatureFlow", WholeEyeVasculatureFlow),
    ("WholeSlideImaging", WholeSlideImaging),
    ("WidthDescriptor", WidthDescriptor),
    ("WinstonLutzPointer", WinstonLutzPointer),
    ("WireMarker", WireMarker),
    ("WorkingCanineAssociationOfCanada", WorkingCanineAssociationOfCanada),
    ("WorkitemAssignmentRejectedByAssignedResource", WorkitemAssignmentRejectedByAssignedResource),
    ("WorkitemExpired", WorkitemExpired),
    ("Workstation", Workstation),
    ("WorldKennelClub", WorldKennelClub),
    ("WorldWideKennelClub", WorldWideKennelClub),
    ("XConcept", XConcept),
    ("XDescentPressure", XDescentPressure),
    ("XOrientation", XOrientation),
    ("XRayAngiography", XRayAngiography),
    ("XRayAttenuation", XRayAttenuation),
    ("XRayAttenuationCoefficient", XRayAttenuationCoefficient),
    ("XRayAttenuatorModelData", XRayAttenuatorModelData),
    ("XRayBeamAttenuator", XRayBeamAttenuator),
    ("XRayBeamAttenuatorModel", XRayBeamAttenuatorModel),
    ("XRayBeamAttenuatorModelReference", XRayBeamAttenuatorModelReference),
    ("XRayBeamAttenuatorModelRegistration", XRayBeamAttenuatorModelRegistration),
    ("XRayDetectorDataAvailable", XRayDetectorDataAvailable),
    ("XRayFilterAluminumEquivalent", XRayFilterAluminumEquivalent),
    ("XRayFilterMaterial", XRayFilterMaterial),
    ("XRayFilterThickness", XRayFilterThickness),
    ("XRayFilterThicknessMaximum", XRayFilterThicknessMaximum),
    ("XRayFilterThicknessMinimum", XRayFilterThicknessMinimum),
    ("XRayFilterType", XRayFilterType),
    ("XRayFilters", XRayFilters),
    ("XRayFluoroscopyOnSignal", XRayFluoroscopyOnSignal),
    ("XRayGrid", XRayGrid),
    ("XRayMechanicalDataAvailable", XRayMechanicalDataAvailable),
    ("XRayModulationType", XRayModulationType),
    ("XRayOnTrigger", XRayOnTrigger),
    ("XRayRadiationDoseReport", XRayRadiationDoseReport),
    ("XRayReadingDevice", XRayReadingDevice),
    ("XRaySourceDataAvailable", XRaySourceDataAvailable),
    ("XRaySourceReferenceCoordinateSystem", XRaySourceReferenceCoordinateSystem),
    ("XRayTomographyImageReconstruction", XRayTomographyImageReconstruction),
    ("XRayTubeCurrent", XRayTubeCurrent),
    ("XmAb557489Zr", XmAb557489Zr),
    ("YBRFULLCBComponent", YBRFULLCBComponent),
    ("YBRFULLCRComponent", YBRFULLCRComponent),
    ("YBRFULLYComponent", YBRFULLYComponent),
    ("YBRICTCBComponent", YBRICTCBComponent),
    ("YBRICTCRComponent", YBRICTCRComponent),
    ("YBRICTYComponent", YBRICTYComponent),
    ("YBRPARTIALCBComponent", YBRPARTIALCBComponent),
    ("YBRPARTIALCRComponent", YBRPARTIALCRComponent),
    ("YBRPARTIALYComponent", YBRPARTIALYComponent),
    ("YBRRCTCBComponent", YBRRCTCBComponent),
    ("YBRRCTCRComponent", YBRRCTCRComponent),
    ("YBRRCTYComponent", YBRRCTYComponent),
    ("YConcept", YConcept),
    ("YDescentPressure", YDescentPressure),
    ("YOrientation", YOrientation),
    ("YellowGreenFilter", YellowGreenFilter),
    ("ZPointPressure", ZPointPressure),
    ("ZScore", ZScore),
    ("ZScoreOfMeasurement", ZScoreOfMeasurement),
    ("ZValueOfLocationOfWaterEquivalentDiameterEstimation", ZValueOfLocationOfWaterEquivalentDiameterEstimation),
    ("Zidere2021", Zidere2021),
    ("ZobelFilter", ZobelFilter),
    ("ZonePercentage", ZonePercentage),
    ("ZoneSizeEntropy", ZoneSizeEntropy),
    ("ZoneSizeNonuniformity", ZoneSizeNonuniformity),
    ("ZoneSizeNonuniformityNormalized", ZoneSizeNonuniformityNormalized),
    ("ZoneSizeVariance", ZoneSizeVariance),
];
