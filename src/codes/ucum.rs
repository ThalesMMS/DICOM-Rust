#![allow(non_upper_case_globals)]
use super::Code;
pub const SCHEME: &str = "UCUM";
const fn c(value: &'static str, meaning: &'static str) -> Code<'static> {
    Code::new(value, SCHEME, meaning)
}
pub const Animals: Code<'static> = c("{animals}", "animals");
pub const ArbitraryUnit: Code<'static> = c("[arb'U]", "arbitrary unit");
pub const BPM: Code<'static> = c("{H.B.}/min", "BPM");
pub const Beats: Code<'static> = c("{beats}", "beats");
pub const Becquerel: Code<'static> = c("Bq", "becquerel");
pub const BecquerelsPerMilliliter: Code<'static> = c("Bq/ml", "Becquerels/milliliter");
pub const BqPerMmol: Code<'static> = c("Bq/mmol", "Bq/mmol");
pub const BqPerMol: Code<'static> = c("Bq/mol", "Bq/mol");
pub const Cages: Code<'static> = c("{cages}", "cages");
pub const Calcifications: Code<'static> = c("{calcifications}", "calcifications");
pub const Centimeter2: Code<'static> = c("cm2", "Centimeter**2");
pub const Centimeter2PerMilliliter: Code<'static> = c("cm2/ml", "Centimeter**2/milliliter");
pub const CentimeterPerSecond: Code<'static> = c("cm/s", "centimeter/second");
pub const Channels: Code<'static> = c("{channels}", "channels");
pub const Cm: Code<'static> = c("cm", "cm");
pub const Cm2PerM2: Code<'static> = c("cm2/m2", "cm2/m2");
pub const Counts: Code<'static> = c("{counts}", "Counts");
pub const CountsPerSecond: Code<'static> = c("{counts}/s", "Counts per second");
pub const CubicCentimeter: Code<'static> = c("cm3", "cubic centimeter");
pub const CubicDecimeter: Code<'static> = c("dm3", "cubic decimeter");
pub const CubicMicrometer: Code<'static> = c("um3", "cubic micrometer");
pub const CubicMillimeter: Code<'static> = c("mm3", "cubic millimeter");
pub const DBMV: Code<'static> = c("dB[mV]", "dB(mV)");
pub const DBPerCmPerMHz: Code<'static> = c("dB/cm/MHz", "dB/cm/MHz");
pub const DBUV: Code<'static> = c("dB[uV]", "dB(µV)");
pub const Day: Code<'static> = c("d", "day");
pub const Decibel: Code<'static> = c("dB", "decibel");
pub const Degree: Code<'static> = c("deg", "Degree");
pub const DegreesCelsius: Code<'static> = c("Cel", "degrees Celsius");
pub const Diopters__diop_: Code<'static> = c("[diop]", "diopters");
pub const Diopters_diop: Code<'static> = c("diop", "diopters");
pub const DynSCm5: Code<'static> = c("dyn.s.cm-5", "dyn.s.cm-5");
pub const DynSCm5PerM2: Code<'static> = c("dyn.s.cm-5/m2", "dyn.s.cm-5/m2");
pub const DynesPerCm2: Code<'static> = c("dyn/cm2", "dynes/cm2");
pub const ElectronDensity: Code<'static> = c("10*23/ml", "Electron Density");
pub const Events: Code<'static> = c("{events}", "events");
pub const FalseNegatives: Code<'static> = c("{false negatives}", "false negatives");
pub const FalsePositives: Code<'static> = c("{false positives}", "false positives");
pub const Findings: Code<'static> = c("{findings}", "findings");
pub const FixationLosses: Code<'static> = c("{fixation losses}", "fixation losses");
pub const Frames: Code<'static> = c("{frames}", "frames");
pub const French: Code<'static> = c("[Ch]", "french");
pub const G: Code<'static> = c("g", "g");
pub const GPerDl: Code<'static> = c("g/dl", "g/dl");
pub const GfM: Code<'static> = c("gf.m", "gf.m");
pub const GfMPerM2: Code<'static> = c("gf.m/m2", "gf.m/m2");
pub const GramPerKg: Code<'static> = c("g/kg", "gram/kg");
pub const GramPerM2: Code<'static> = c("g/m2", "gram/m2");
pub const Gray: Code<'static> = c("Gy", "Gray");
pub const GyM2: Code<'static> = c("Gy.m2", "Gy.m2");
pub const GyPerS: Code<'static> = c("Gy/s", "Gy/s");
pub const HounsfieldUnit: Code<'static> = c("[hnsf'U]", "Hounsfield unit");
pub const Hour: Code<'static> = c("h", "hour");
pub const HousingUnits: Code<'static> = c("{housing units}", "housing units");
pub const Hz: Code<'static> = c("Hz", "Hz");
pub const Images: Code<'static> = c("{images}", "images");
pub const Instances: Code<'static> = c("{instances}", "instances");
pub const InternationalUnit: Code<'static> = c("[iU]", "International unit");
pub const KHz: Code<'static> = c("kHz", "kHz");
pub const KPa: Code<'static> = c("kPa", "kPa");
pub const Kg: Code<'static> = c("kg", "kg");
pub const KgPerM2: Code<'static> = c("kg/m2", "kg/m2");
pub const KiloelectronvoltPerMicrometer: Code<'static> = c("keV/um", "Kiloelectronvolt/micrometer");
pub const Kilovolt: Code<'static> = c("kV", "Kilovolt");
pub const KmPerH: Code<'static> = c("km/h", "km/h");
pub const L: Code<'static> = c("l", "l");
pub const LPerMin: Code<'static> = c("l/min", "l/min");
pub const LPerMinPerM2: Code<'static> = c("l/min/m2", "l/min/m2");
pub const LPerMmolPerS: Code<'static> = c("l/mmol/s", "l/mmol/s");
pub const Lesions: Code<'static> = c("{lesions}", "lesions");
pub const M: Code<'static> = c("m", "m");
pub const M2: Code<'static> = c("m2", "m2");
pub const MA: Code<'static> = c("mA", "mA");
pub const METS: Code<'static> = c("[MET]", "METS");
pub const MGy: Code<'static> = c("mGy", "mGy");
pub const MGyCm: Code<'static> = c("mGy.cm", "mGy.cm");
pub const MGyCm2: Code<'static> = c("mGy.cm2", "mGy.cm2");
pub const MGyPerMAS: Code<'static> = c("mGy/mA.s", "mGy/mA.s");
pub const MPerS: Code<'static> = c("m/s", "m/s");
pub const MPerSPerKHz: Code<'static> = c("m/s/kHz", "m/s/kHz");
pub const MSv: Code<'static> = c("mSv", "mSv");
pub const MSvPerMGyCm: Code<'static> = c("mSv/mGy.cm", "mSv/mGy.cm");
pub const MV: Code<'static> = c("mV", "mV");
pub const Megabecquerel: Code<'static> = c("MBq", "megabecquerel");
pub const Megaelectronvolt: Code<'static> = c("MeV", "Megaelectronvolt");
pub const Megavolt: Code<'static> = c("MV", "Megavolt");
pub const MeqPerDl: Code<'static> = c("meq/dl", "meq/dl");
pub const Mg: Code<'static> = c("mg", "mg");
pub const MgPerCm3: Code<'static> = c("mg/cm3", "mg/cm3");
pub const MgPerDl: Code<'static> = c("mg/dl", "mg/dl");
pub const MgPerHnsfUCm3: Code<'static> = c("mg/[hnsf'U].cm3", "mg/[hnsf'U].cm3");
pub const MgPerKg: Code<'static> = c("mg/kg", "mg/kg");
pub const MgPerMl: Code<'static> = c("mg/ml", "mg/ml");
pub const Micrometer: Code<'static> = c("um", "micrometer");
pub const MicromolePerMilliliter: Code<'static> = c("umol/ml", "Micromole/milliliter");
pub const MicromolePerMinutePerMilliliter: Code<'static> = c("umol/min/ml", "Micromole/minute/milliliter");
pub const MilliTesla: Code<'static> = c("mT", "milliTesla");
pub const Millibar: Code<'static> = c("mbar", "millibar");
pub const Millicurie: Code<'static> = c("mCi", "millicurie");
pub const MilligramsPerMinutePerMilliliter: Code<'static> = c("mg/min/ml", "Milligrams/minute/milliliter");
pub const MilliliterPer100GramPerMinute: Code<'static> = c("ml/[100]g/min", "milliliter per 100 gram per minute");
pub const MilliliterPer100Milliliter: Code<'static> = c("ml/[100]ml", "milliliter per 100 milliliter");
pub const MilliliterPerGram: Code<'static> = c("ml/g", "Milliliter/gram");
pub const MilliliterPerMinute: Code<'static> = c("ml/min", "milliliter per minute");
pub const MilliliterPerMinutePerGram: Code<'static> = c("ml/min/g", "Milliliter/minute/gram");
pub const MilliliterPerSecond: Code<'static> = c("ml/s", "milliliter per second");
pub const MillimeterPerSecond: Code<'static> = c("mm/s", "millimeter/second");
pub const MillimolesPerKgWetWeight: Code<'static> = c("mmol/kg{WetWeight}", "millimoles per kg wet weight");
pub const Millisecond: Code<'static> = c("ms", "millisecond");
pub const Minute: Code<'static> = c("min", "minute");
pub const Ml: Code<'static> = c("ml", "ml");
pub const MlPerBPM: Code<'static> = c("ml/{H.B.}/min", "ml/BPM");
pub const MlPerDl: Code<'static> = c("ml/dl", "ml/dl");
pub const MlPerKg: Code<'static> = c("ml/kg", "ml/kg");
pub const MlPerM2: Code<'static> = c("ml/m2", "ml/m2");
pub const MlPerMinPer173m2: Code<'static> = c("ml/min{1.73_m2}", "ml/min/1.73m2");
pub const MlPerMinPerBPM: Code<'static> = c("ml/min/{H.B.}/min", "(ml/min) /BPM");
pub const MlPerMinPerM2: Code<'static> = c("ml/min/m2", "(ml/min) /m2");
pub const MlPerSPerBPM: Code<'static> = c("ml/s/{H.B.}/min", "(ml/s) /BPM");
pub const MlPerSPerM2: Code<'static> = c("ml/s/m2", "(ml/s) /m2");
pub const Mm: Code<'static> = c("mm", "mm");
pub const Mm2PerS: Code<'static> = c("mm2/s", "mm2/s");
pub const MmHGS2PerCm2: Code<'static> = c("mm[Hg].s2/cm2", "mmHG.s2/cm2");
pub const MmHGSPerCm: Code<'static> = c("mm[Hg].s/cm", "mmHG.s/cm");
pub const MmHg: Code<'static> = c("mm[Hg]", "mmHg");
pub const MmHgBPM: Code<'static> = c("mm[Hg].{H.B.}/min", "mmHg.BPM");
pub const MmHgPerS: Code<'static> = c("mm[Hg]/s", "mmHg/s");
pub const MmHgPerSPerS: Code<'static> = c("mm[Hg]/s2", "mmHg/s/s");
pub const MmPerPixel: Code<'static> = c("mm/{pixel}", "mm/pixel");
pub const MmolPerL: Code<'static> = c("mmol/l", "mmol/l");
pub const MonitorUnits: Code<'static> = c("{MU}", "Monitor Units");
pub const MonitorUnitsPerSecond: Code<'static> = c("{MU}/s", "Monitor Units/Second");
pub const Month: Code<'static> = c("mo", "month");
pub const MosmPerKg: Code<'static> = c("mosm/kg", "mosm/kg");
pub const Mph: Code<'static> = c("[mi_i]/h", "mph");
pub const N106Mm2PerS: Code<'static> = c("10-6.mm2/s", "10-6.mm2/s");
pub const N11: Code<'static> = c("{-1:1}", "{-1:1}");
pub const Needles: Code<'static> = c("{needles}", "needles");
pub const Newton: Code<'static> = c("N", "Newton");
pub const Nm: Code<'static> = c("nm", "nm");
pub const NoUnits: Code<'static> = c("1", "no units");
pub const Nodes: Code<'static> = c("{nodes}", "nodes");
pub const NumberParticlesPer100GramOfTissue: Code<'static> = c("{Particles}/[100]g{Tissue}", "number particles per 100 gram of tissue");
pub const PH: Code<'static> = c("[pH]", "pH");
pub const PRU: Code<'static> = c("[PRU]", "P.R.U.");
pub const PRUPerM2: Code<'static> = c("[PRU]/m2", "P.R.U./m2");
pub const Pa: Code<'static> = c("Pa", "Pa");
pub const Passes: Code<'static> = c("{passes}", "passes");
pub const PerCentimeter: Code<'static> = c("/cm", "/Centimeter");
pub const PerHour: Code<'static> = c("/h", "/hour");
pub const PerMin: Code<'static> = c("/min", "/min");
pub const PerS: Code<'static> = c("/s", "/s");
pub const PerU: Code<'static> = c("/u", "/u");
pub const Percent: Code<'static> = c("%", "Percent");
pub const PercentPerBPM: Code<'static> = c("%/{H.B.}/min", "%/BPM");
pub const PercentPerM2: Code<'static> = c("%/m2", "%/m2");
pub const Pixels: Code<'static> = c("{pixels}", "pixels");
pub const Planes: Code<'static> = c("{planes}", "planes");
pub const Ppm: Code<'static> = c("ppm", "ppm");
pub const PrismDiopters: Code<'static> = c("[p'diop]", "prism diopters");
pub const ProportionalToCounts: Code<'static> = c("{propcounts}", "Proportional to counts");
pub const ProportionalToCountsPerSecond: Code<'static> = c("{propcounts}/s", "Proportional to counts per second");
pub const PulsePerS: Code<'static> = c("{pulse}/s", "pulse/s");
pub const Racks: Code<'static> = c("{racks}", "racks");
pub const RadPerS: Code<'static> = c("rad/s", "rad/s");
pub const Radian: Code<'static> = c("rad", "Radian");
pub const Range0100: Code<'static> = c("{0:100}", "range:0:100");
pub const Range02: Code<'static> = c("{0:2}", "range 0:2");
pub const Range04: Code<'static> = c("{0:4}", "range 0:4");
pub const Range0N: Code<'static> = c("{0:n}", "range: 0:n");
pub const Range110: Code<'static> = c("{1:10}", "range 1:10");
pub const Range1N: Code<'static> = c("{1:n}", "range: 1:n");
pub const Ratio: Code<'static> = c("{ratio}", "ratio");
pub const SPerMin: Code<'static> = c("s/min", "s/min");
pub const Samples: Code<'static> = c("{samples}", "samples");
pub const Second: Code<'static> = c("s", "second");
pub const SecondPerSquareMillimeter: Code<'static> = c("s/mm2", "second per square millimeter");
pub const Series: Code<'static> = c("{series}", "series");
pub const Specimens: Code<'static> = c("{specimens}", "specimens");
pub const SquareMicrometer: Code<'static> = c("um2", "square micrometer");
pub const SquareMillimeter: Code<'static> = c("mm2", "square millimeter");
pub const Stage: Code<'static> = c("{stage}", "stage");
pub const StandardDeviations: Code<'static> = c("{sd}", "Standard Deviations");
pub const StandardizedUptakeValueBodySurfaceArea: Code<'static> = c("cm2/ml{SUVbsa}", "Standardized Uptake Value body surface area");
pub const StandardizedUptakeValueBodyWeight: Code<'static> = c("g/ml{SUVbw}", "Standardized Uptake Value body weight");
pub const StandardizedUptakeValueIdealBodyWeight: Code<'static> = c("g/ml{SUVibw}", "Standardized Uptake Value ideal body weight");
pub const StandardizedUptakeValueLeanBodyMassJames: Code<'static> = c("g/ml{SUVlbm}", "Standardized Uptake Value lean body mass (James)");
pub const StandardizedUptakeValueLeanBodyMassJames128Multiplier: Code<'static> = c("g/ml{SUVlbm(James128)}", "Standardized Uptake Value lean body mass (James 128 multiplier)");
pub const StandardizedUptakeValueLeanBodyMassJanma: Code<'static> = c("g/ml{SUVlbm(Janma)}", "Standardized Uptake Value lean body mass (Janma)");
pub const Step: Code<'static> = c("{step}", "step");
pub const Sv: Code<'static> = c("Sv", "Sv");
pub const Territories: Code<'static> = c("{territories}", "territories");
pub const Trials: Code<'static> = c("{trials}", "trials");
pub const UAS: Code<'static> = c("uA.s", "uA.s");
pub const UV: Code<'static> = c("uV", "µV");
pub const UgPerKgPerMin: Code<'static> = c("ug/kg/min", "ug/kg/min");
pub const Um2PerMs: Code<'static> = c("um2/ms", "um2/ms");
pub const Um2PerS: Code<'static> = c("um2/s", "um2/s");
pub const WattPerKilogram: Code<'static> = c("W/kg", "Watt/kilogram");
pub const Watts: Code<'static> = c("W", "Watts");
pub const Week: Code<'static> = c("wk", "week");
pub const WoodU: Code<'static> = c("[wood'U]", "Wood U");
pub const WoodUPerM2: Code<'static> = c("[wood'U]/m2", "Wood U/m2");
pub const XRaySources: Code<'static> = c("{X-Ray sources}", "X-Ray sources");
pub const Year: Code<'static> = c("a", "year");
pub fn by_name(name: &str) -> Option<Code<'static>> {
    super::find_named(&NAMED, name)
}
#[allow(deprecated)]
static NAMED: [(&str, Code<'static>); 205] = [
    ("Animals", Animals),
    ("ArbitraryUnit", ArbitraryUnit),
    ("BPM", BPM),
    ("Beats", Beats),
    ("Becquerel", Becquerel),
    ("BecquerelsPerMilliliter", BecquerelsPerMilliliter),
    ("BqPerMmol", BqPerMmol),
    ("BqPerMol", BqPerMol),
    ("Cages", Cages),
    ("Calcifications", Calcifications),
    ("Centimeter2", Centimeter2),
    ("Centimeter2PerMilliliter", Centimeter2PerMilliliter),
    ("CentimeterPerSecond", CentimeterPerSecond),
    ("Channels", Channels),
    ("Cm", Cm),
    ("Cm2PerM2", Cm2PerM2),
    ("Counts", Counts),
    ("CountsPerSecond", CountsPerSecond),
    ("CubicCentimeter", CubicCentimeter),
    ("CubicDecimeter", CubicDecimeter),
    ("CubicMicrometer", CubicMicrometer),
    ("CubicMillimeter", CubicMillimeter),
    ("DBMV", DBMV),
    ("DBPerCmPerMHz", DBPerCmPerMHz),
    ("DBUV", DBUV),
    ("Day", Day),
    ("Decibel", Decibel),
    ("Degree", Degree),
    ("DegreesCelsius", DegreesCelsius),
    ("Diopters__diop_", Diopters__diop_),
    ("Diopters_diop", Diopters_diop),
    ("DynSCm5", DynSCm5),
    ("DynSCm5PerM2", DynSCm5PerM2),
    ("DynesPerCm2", DynesPerCm2),
    ("ElectronDensity", ElectronDensity),
    ("Events", Events),
    ("FalseNegatives", FalseNegatives),
    ("FalsePositives", FalsePositives),
    ("Findings", Findings),
    ("FixationLosses", FixationLosses),
    ("Frames", Frames),
    ("French", French),
    ("G", G),
    ("GPerDl", GPerDl),
    ("GfM", GfM),
    ("GfMPerM2", GfMPerM2),
    ("GramPerKg", GramPerKg),
    ("GramPerM2", GramPerM2),
    ("Gray", Gray),
    ("GyM2", GyM2),
    ("GyPerS", GyPerS),
    ("HounsfieldUnit", HounsfieldUnit),
    ("Hour", Hour),
    ("HousingUnits", HousingUnits),
    ("Hz", Hz),
    ("Images", Images),
    ("Instances", Instances),
    ("InternationalUnit", InternationalUnit),
    ("KHz", KHz),
    ("KPa", KPa),
    ("Kg", Kg),
    ("KgPerM2", KgPerM2),
    ("KiloelectronvoltPerMicrometer", KiloelectronvoltPerMicrometer),
    ("Kilovolt", Kilovolt),
    ("KmPerH", KmPerH),
    ("L", L),
    ("LPerMin", LPerMin),
    ("LPerMinPerM2", LPerMinPerM2),
    ("LPerMmolPerS", LPerMmolPerS),
    ("Lesions", Lesions),
    ("M", M),
    ("M2", M2),
    ("MA", MA),
    ("METS", METS),
    ("MGy", MGy),
    ("MGyCm", MGyCm),
    ("MGyCm2", MGyCm2),
    ("MGyPerMAS", MGyPerMAS),
    ("MPerS", MPerS),
    ("MPerSPerKHz", MPerSPerKHz),
    ("MSv", MSv),
    ("MSvPerMGyCm", MSvPerMGyCm),
    ("MV", MV),
    ("Megabecquerel", Megabecquerel),
    ("Megaelectronvolt", Megaelectronvolt),
    ("Megavolt", Megavolt),
    ("MeqPerDl", MeqPerDl),
    ("Mg", Mg),
    ("MgPerCm3", MgPerCm3),
    ("MgPerDl", MgPerDl),
    ("MgPerHnsfUCm3", MgPerHnsfUCm3),
    ("MgPerKg", MgPerKg),
    ("MgPerMl", MgPerMl),
    ("Micrometer", Micrometer),
    ("MicromolePerMilliliter", MicromolePerMilliliter),
    ("MicromolePerMinutePerMilliliter", MicromolePerMinutePerMilliliter),
    ("MilliTesla", MilliTesla),
    ("Millibar", Millibar),
    ("Millicurie", Millicurie),
    ("MilligramsPerMinutePerMilliliter", MilligramsPerMinutePerMilliliter),
    ("MilliliterPer100GramPerMinute", MilliliterPer100GramPerMinute),
    ("MilliliterPer100Milliliter", MilliliterPer100Milliliter),
    ("MilliliterPerGram", MilliliterPerGram),
    ("MilliliterPerMinute", MilliliterPerMinute),
    ("MilliliterPerMinutePerGram", MilliliterPerMinutePerGram),
    ("MilliliterPerSecond", MilliliterPerSecond),
    ("MillimeterPerSecond", MillimeterPerSecond),
    ("MillimolesPerKgWetWeight", MillimolesPerKgWetWeight),
    ("Millisecond", Millisecond),
    ("Minute", Minute),
    ("Ml", Ml),
    ("MlPerBPM", MlPerBPM),
    ("MlPerDl", MlPerDl),
    ("MlPerKg", MlPerKg),
    ("MlPerM2", MlPerM2),
    ("MlPerMinPer173m2", MlPerMinPer173m2),
    ("MlPerMinPerBPM", MlPerMinPerBPM),
    ("MlPerMinPerM2", MlPerMinPerM2),
    ("MlPerSPerBPM", MlPerSPerBPM),
    ("MlPerSPerM2", MlPerSPerM2),
    ("Mm", Mm),
    ("Mm2PerS", Mm2PerS),
    ("MmHGS2PerCm2", MmHGS2PerCm2),
    ("MmHGSPerCm", MmHGSPerCm),
    ("MmHg", MmHg),
    ("MmHgBPM", MmHgBPM),
    ("MmHgPerS", MmHgPerS),
    ("MmHgPerSPerS", MmHgPerSPerS),
    ("MmPerPixel", MmPerPixel),
    ("MmolPerL", MmolPerL),
    ("MonitorUnits", MonitorUnits),
    ("MonitorUnitsPerSecond", MonitorUnitsPerSecond),
    ("Month", Month),
    ("MosmPerKg", MosmPerKg),
    ("Mph", Mph),
    ("N106Mm2PerS", N106Mm2PerS),
    ("N11", N11),
    ("Needles", Needles),
    ("Newton", Newton),
    ("Nm", Nm),
    ("NoUnits", NoUnits),
    ("Nodes", Nodes),
    ("NumberParticlesPer100GramOfTissue", NumberParticlesPer100GramOfTissue),
    ("PH", PH),
    ("PRU", PRU),
    ("PRUPerM2", PRUPerM2),
    ("Pa", Pa),
    ("Passes", Passes),
    ("PerCentimeter", PerCentimeter),
    ("PerHour", PerHour),
    ("PerMin", PerMin),
    ("PerS", PerS),
    ("PerU", PerU),
    ("Percent", Percent),
    ("PercentPerBPM", PercentPerBPM),
    ("PercentPerM2", PercentPerM2),
    ("Pixels", Pixels),
    ("Planes", Planes),
    ("Ppm", Ppm),
    ("PrismDiopters", PrismDiopters),
    ("ProportionalToCounts", ProportionalToCounts),
    ("ProportionalToCountsPerSecond", ProportionalToCountsPerSecond),
    ("PulsePerS", PulsePerS),
    ("Racks", Racks),
    ("RadPerS", RadPerS),
    ("Radian", Radian),
    ("Range0100", Range0100),
    ("Range02", Range02),
    ("Range04", Range04),
    ("Range0N", Range0N),
    ("Range110", Range110),
    ("Range1N", Range1N),
    ("Ratio", Ratio),
    ("SPerMin", SPerMin),
    ("Samples", Samples),
    ("Second", Second),
    ("SecondPerSquareMillimeter", SecondPerSquareMillimeter),
    ("Series", Series),
    ("Specimens", Specimens),
    ("SquareMicrometer", SquareMicrometer),
    ("SquareMillimeter", SquareMillimeter),
    ("Stage", Stage),
    ("StandardDeviations", StandardDeviations),
    ("StandardizedUptakeValueBodySurfaceArea", StandardizedUptakeValueBodySurfaceArea),
    ("StandardizedUptakeValueBodyWeight", StandardizedUptakeValueBodyWeight),
    ("StandardizedUptakeValueIdealBodyWeight", StandardizedUptakeValueIdealBodyWeight),
    ("StandardizedUptakeValueLeanBodyMassJames", StandardizedUptakeValueLeanBodyMassJames),
    ("StandardizedUptakeValueLeanBodyMassJames128Multiplier", StandardizedUptakeValueLeanBodyMassJames128Multiplier),
    ("StandardizedUptakeValueLeanBodyMassJanma", StandardizedUptakeValueLeanBodyMassJanma),
    ("Step", Step),
    ("Sv", Sv),
    ("Territories", Territories),
    ("Trials", Trials),
    ("UAS", UAS),
    ("UV", UV),
    ("UgPerKgPerMin", UgPerKgPerMin),
    ("Um2PerMs", Um2PerMs),
    ("Um2PerS", Um2PerS),
    ("WattPerKilogram", WattPerKilogram),
    ("Watts", Watts),
    ("Week", Week),
    ("WoodU", WoodU),
    ("WoodUPerM2", WoodUPerM2),
    ("XRaySources", XRaySources),
    ("Year", Year),
];
