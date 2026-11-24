#![allow(non_upper_case_globals)]
use super::Code;
pub const SCHEME: &str = "UMLS";
const fn c(value: &'static str, meaning: &'static str) -> Code<'static> {
    Code::new(value, SCHEME, meaning)
}
pub const Alphaxalone: Code<'static> = c("C0051482", "Alphaxalone");
pub const AnesthesiaRecoveryPeriod: Code<'static> = c("C0002908", "Anesthesia recovery period");
pub const Antihypoglycemic: Code<'static> = c("C1579431", "Antihypoglycemic");
pub const AorticAnnulusArea: Code<'static> = c("C2059685", "Aortic annulus area");
pub const AorticSinusOfValsalvaDiameter: Code<'static> = c("C2059455", "Aortic sinus of valsalva diameter");
pub const Baseline: Code<'static> = c("C1442488", "Baseline");
pub const CoefficientOfVariance: Code<'static> = c("C0681921", "Coefficient of Variance");
pub const CommercialProduct: Code<'static> = c("C1547887", "Commercial product");
pub const ConsultingPhysician: Code<'static> = c("C1441532", "Consulting Physician");
pub const DistalPhalanx: Code<'static> = c("C3669027", "Distal phalanx");
pub const Duration: Code<'static> = c("C0449238", "Duration");
pub const EdotreotideGa68: Code<'static> = c("C2713594", "Edotreotide Ga^68^");
pub const EthidiumBromide: Code<'static> = c("C0019873", "Ethidium Bromide");
pub const FeedingMethod: Code<'static> = c("C0015746", "Feeding method");
pub const Fluanisone: Code<'static> = c("C0060473", "Fluanisone");
pub const FluoroetanidazoleF18: Code<'static> = c("C1541539", "Fluoroetanidazole F^18^");
pub const FluoropropylDihydrotetrabenazineF18: Code<'static> = c("C2934038", "Fluoropropyl-dihydrotetrabenazine F^18^");
pub const H2Antagonist: Code<'static> = c("C0019593", "H2 antagonist");
pub const HeartSize: Code<'static> = c("C0744689", "Heart size");
pub const HeatLamp: Code<'static> = c("C0181514", "Heat lamp");
pub const Heating: Code<'static> = c("C0018851", "Heating");
pub const ISO1F18: Code<'static> = c("C2981788", "ISO-1 F^18^");
pub const ImplantationOfMitralValveLeafletClip: Code<'static> = c("C2921037", "Implantation of mitral valve leaflet clip");
pub const InfantOfMotherWithGestationalDiabetes: Code<'static> = c("C0456029", "Infant of mother with gestational diabetes");
pub const Intern: Code<'static> = c("C1144859", "Intern");
pub const IntoleranceToAnticoagulation: Code<'static> = c("C3468959", "Intolerance to anticoagulation");
pub const InverseRatioVentilation: Code<'static> = c("C2223982", "Inverse ratio ventilation");
pub const KevlarAramidFiber: Code<'static> = c("C0064329", "Kevlar Aramid Fiber");
pub const LeftAtrialAppendageOcclusion: Code<'static> = c("C3275093", "Left atrial appendage occlusion");
pub const LeftAtrialDilation: Code<'static> = c("C4015487", "Left atrial dilation");
pub const MK6240F18: Code<'static> = c("C4506764", "MK-6240 F^18^");
pub const ManufacturerName: Code<'static> = c("C0947322", "Manufacturer Name");
pub const Mastectomy: Code<'static> = c("C0024881", "Mastectomy");
pub const MedicalPhysicist: Code<'static> = c("C1708969", "Medical Physicist");
pub const Metomidate: Code<'static> = c("C0025856", "Metomidate");
pub const MiddleEasternOrNorthAfrican: Code<'static> = c("C5690844", "Middle Eastern or North African");
pub const MobileSkinLesion: Code<'static> = c("C2071496", "Mobile skin lesion");
pub const Nadir: Code<'static> = c("C1708760", "Nadir");
pub const NarcoticAnalgesic: Code<'static> = c("C0027409", "Narcotic analgesic");
pub const Neuroradiology: Code<'static> = c("C2183225", "Neuroradiology");
pub const PI2620F18: Code<'static> = c("C5433257", "PI-2620 F^18^");
pub const PK1119511C: Code<'static> = c("C1609883", "PK11195 ^11^C");
pub const PercutaneousClosureOfAtrialSeptalDefect: Code<'static> = c("C0844084", "Percutaneous closure of atrial septal defect");
pub const Pretreatment: Code<'static> = c("C3539075", "Pretreatment");
pub const QualitativeEvaluations: Code<'static> = c("C0034375", "Qualitative Evaluations");
pub const RadiationPhysicist: Code<'static> = c("C2985483", "Radiation Physicist");
pub const Referring: Code<'static> = c("C1709880", "Referring");
pub const RestrictedDiet: Code<'static> = c("C0425422", "Restricted diet");
pub const RoomAir: Code<'static> = c("C3846005", "Room air");
pub const RootMeanSquare: Code<'static> = c("C2347976", "Root Mean Square");
pub const Sonographer: Code<'static> = c("C1954848", "Sonographer");
pub const THK5317F18: Code<'static> = c("C4550127", "THK5317 F^18^");
pub const THK5351F18: Code<'static> = c("C4279748", "THK5351 F^18^");
pub const Thermography: Code<'static> = c("C0039810", "Thermography");
pub const TimePoint: Code<'static> = c("C2348792", "Time Point");
pub const TransgenicAnimal: Code<'static> = c("C0003069", "Transgenic animal");
pub const UCBJC11: Code<'static> = c("C4506788", "UCB-J C^11^");
pub const UnknownPrimaryNeoplasiaSite: Code<'static> = c("C0221297", "unknown primary neoplasia site");
pub const Unscheduled: Code<'static> = c("C1699701", "Unscheduled");
pub const Variance: Code<'static> = c("C1711260", "Variance");
pub fn by_name(name: &str) -> Option<Code<'static>> {
    super::find_named(&NAMED, name)
}
#[allow(deprecated)]
static NAMED: [(&str, Code<'static>); 60] = [
    ("Alphaxalone", Alphaxalone),
    ("AnesthesiaRecoveryPeriod", AnesthesiaRecoveryPeriod),
    ("Antihypoglycemic", Antihypoglycemic),
    ("AorticAnnulusArea", AorticAnnulusArea),
    ("AorticSinusOfValsalvaDiameter", AorticSinusOfValsalvaDiameter),
    ("Baseline", Baseline),
    ("CoefficientOfVariance", CoefficientOfVariance),
    ("CommercialProduct", CommercialProduct),
    ("ConsultingPhysician", ConsultingPhysician),
    ("DistalPhalanx", DistalPhalanx),
    ("Duration", Duration),
    ("EdotreotideGa68", EdotreotideGa68),
    ("EthidiumBromide", EthidiumBromide),
    ("FeedingMethod", FeedingMethod),
    ("Fluanisone", Fluanisone),
    ("FluoroetanidazoleF18", FluoroetanidazoleF18),
    ("FluoropropylDihydrotetrabenazineF18", FluoropropylDihydrotetrabenazineF18),
    ("H2Antagonist", H2Antagonist),
    ("HeartSize", HeartSize),
    ("HeatLamp", HeatLamp),
    ("Heating", Heating),
    ("ISO1F18", ISO1F18),
    ("ImplantationOfMitralValveLeafletClip", ImplantationOfMitralValveLeafletClip),
    ("InfantOfMotherWithGestationalDiabetes", InfantOfMotherWithGestationalDiabetes),
    ("Intern", Intern),
    ("IntoleranceToAnticoagulation", IntoleranceToAnticoagulation),
    ("InverseRatioVentilation", InverseRatioVentilation),
    ("KevlarAramidFiber", KevlarAramidFiber),
    ("LeftAtrialAppendageOcclusion", LeftAtrialAppendageOcclusion),
    ("LeftAtrialDilation", LeftAtrialDilation),
    ("MK6240F18", MK6240F18),
    ("ManufacturerName", ManufacturerName),
    ("Mastectomy", Mastectomy),
    ("MedicalPhysicist", MedicalPhysicist),
    ("Metomidate", Metomidate),
    ("MiddleEasternOrNorthAfrican", MiddleEasternOrNorthAfrican),
    ("MobileSkinLesion", MobileSkinLesion),
    ("Nadir", Nadir),
    ("NarcoticAnalgesic", NarcoticAnalgesic),
    ("Neuroradiology", Neuroradiology),
    ("PI2620F18", PI2620F18),
    ("PK1119511C", PK1119511C),
    ("PercutaneousClosureOfAtrialSeptalDefect", PercutaneousClosureOfAtrialSeptalDefect),
    ("Pretreatment", Pretreatment),
    ("QualitativeEvaluations", QualitativeEvaluations),
    ("RadiationPhysicist", RadiationPhysicist),
    ("Referring", Referring),
    ("RestrictedDiet", RestrictedDiet),
    ("RoomAir", RoomAir),
    ("RootMeanSquare", RootMeanSquare),
    ("Sonographer", Sonographer),
    ("THK5317F18", THK5317F18),
    ("THK5351F18", THK5351F18),
    ("Thermography", Thermography),
    ("TimePoint", TimePoint),
    ("TransgenicAnimal", TransgenicAnimal),
    ("UCBJC11", UCBJC11),
    ("UnknownPrimaryNeoplasiaSite", UnknownPrimaryNeoplasiaSite),
    ("Unscheduled", Unscheduled),
    ("Variance", Variance),
];
