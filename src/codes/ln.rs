#![allow(non_upper_case_globals)]
use super::Code;
pub const SCHEME: &str = "LN";
const fn c(value: &'static str, meaning: &'static str) -> Code<'static> {
    Code::new(value, SCHEME, meaning)
}
pub const ACASUM2000: Code<'static> = c("33072-0", "AC, ASUM 2000");
pub const ACByGAASUM2000: Code<'static> = c("33145-4", "AC by GA, ASUM 2000");
pub const ACByGAHadlock1984: Code<'static> = c("33146-2", "AC by GA, Hadlock 1984");
pub const ACByGAMerz1988: Code<'static> = c("33148-8", "AC by GA, Merz 1988");
pub const ACByGAShinozuka1996: Code<'static> = c("33149-6", "AC by GA, Shinozuka 1996");
pub const ACCampbell1975: Code<'static> = c("11889-3", "AC, Campbell 1975");
pub const ACDerivedByGAChitty1994: Code<'static> = c("33546-3", "AC (derived) by GA, Chitty 1994");
pub const ACHadlock1984: Code<'static> = c("11892-7", "AC, Hadlock 1984");
pub const ACHansmann1985: Code<'static> = c("33073-8", "AC, Hansmann1985");
pub const ACInterval: Code<'static> = c("59103-2", "A-C Interval");
pub const ACJeanty1982: Code<'static> = c("33537-2", "AC, Jeanty 1982");
pub const ACJeanty1984: Code<'static> = c("11893-5", "AC, Jeanty 1984");
pub const ACLessoway1998: Code<'static> = c("33074-6", "AC, Lessoway 1998");
pub const ACMeasuredByGAChitty1994: Code<'static> = c("33147-0", "AC (measured) by GA, Chitty 1994");
pub const ACMertz1988: Code<'static> = c("33075-3", "AC, Mertz 1988");
pub const ACShinozuka1996: Code<'static> = c("33076-1", "AC, Shinozuka 1996");
pub const APADTAD: Code<'static> = c("33191-8", "APAD * TAD");
pub const APAbdominalDiameterLessoway1998: Code<'static> = c("33077-9", "A-P Abdominal Diameter, Lessoway 1998");
pub const AWaveAmplitude: Code<'static> = c("59100-8", "A-Wave Amplitude");
pub const AWaveDuration: Code<'static> = c("59105-7", "A-Wave Duration");
pub const AWavePeakVelocity: Code<'static> = c("59081-0", "A-Wave Peak Velocity");
pub const AbdominalCircumference: Code<'static> = c("11979-2", "Abdominal Circumference");
pub const AbdominalWallObservation: Code<'static> = c("12030-3", "Abdominal wall observation");
pub const Abortus: Code<'static> = c("11612-9", "Abortus");
pub const AccelerationIndex: Code<'static> = c("20167-3", "Acceleration Index");
pub const AccelerationTime: Code<'static> = c("20168-1", "Acceleration Time");
pub const Addendum: Code<'static> = c("55107-7", "Addendum");
pub const AliasVelocity: Code<'static> = c("59130-5", "Alias velocity");
pub const AmnioticFluidIndex: Code<'static> = c("11627-7", "Amniotic Fluid Index");
pub const AmnioticFluidVolume: Code<'static> = c("11630-1", "Amniotic Fluid Volume");
pub const AnteriorHornLateralVentricularWidth: Code<'static> = c("33197-5", "Anterior Horn Lateral ventricular width");
pub const AnteriorMitralValveLeafletLength: Code<'static> = c("17997-8", "Anterior mitral valve leaflet length");
pub const AnteriorPosteriorAbdominalDiameter: Code<'static> = c("11818-2", "Anterior-Posterior Abdominal Diameter");
pub const AnteriorPosteriorTrunkDiameter: Code<'static> = c("11819-0", "Anterior-Posterior Trunk Diameter");
pub const AortaSinotubularJunctionDiameterBy2D: Code<'static> = c("82339-3", "Aorta sinotubular junction diameter by 2D");
pub const AortaSinotubularJunctionDiameterByMMode: Code<'static> = c("82338-5", "Aorta sinotubular junction diameter by M-mode");
pub const AorticAnnulusDiameter: Code<'static> = c("79940-3", "Aortic annulus diameter");
pub const AorticArchDiameter: Code<'static> = c("18011-7", "Aortic Arch Diameter");
pub const AorticIsthmusDiameter: Code<'static> = c("18014-1", "Aortic Isthmus Diameter");
pub const AorticRegurgitantFlow: Code<'static> = c("79941-1", "Aortic regurgitant flow");
pub const AorticRegurgitantFraction: Code<'static> = c("79942-9", "Aortic regurgitant fraction");
pub const AorticRegurgitantJetAreaPerLVOTAreaPercent: Code<'static> = c("79943-7", "Aortic regurgitant jet area/LVOT area %");
pub const AorticRegurgitantJetWidthPerLVOTWidthPercent: Code<'static> = c("79944-5", "Aortic regurgitant jet width/LVOT width %");
pub const AorticRegurgitationPISARadius: Code<'static> = c("79945-2", "Aortic regurgitation PISA radius");
pub const AorticRegurgitationPISAVelocity: Code<'static> = c("79946-0", "Aortic regurgitation PISA velocity");
pub const AorticRegurgitationPressureHalfTime: Code<'static> = c("79947-8", "Aortic regurgitation pressure half-time");
pub const AorticRegurgitationVTI: Code<'static> = c("79952-8", "Aortic regurgitation VTI");
pub const AorticRegurgitationVenaContractaWidth: Code<'static> = c("79948-6", "Aortic regurgitation vena contracta width");
pub const AorticRegurgitationVmax: Code<'static> = c("79949-4", "Aortic regurgitation Vmax");
pub const AorticRegurgitationVolumeContinuityVTI: Code<'static> = c("79950-2", "Aortic regurgitation volume (Continuity VTI)");
pub const AorticRegurgitationVolumePISA: Code<'static> = c("79951-0", "Aortic regurgitation volume (PISA)");
pub const AorticRootDiameterPerBSA: Code<'static> = c("79954-4", "Aortic root diameter / BSA");
pub const AorticRootDiameter_18015_8: Code<'static> = c("18015-8", "Aortic Root Diameter");
pub const AorticRootDiameter_79953_6: Code<'static> = c("79953-6", "Aortic root diameter");
pub const AorticRootOxygenContent: Code<'static> = c("10232-7", "Aortic Root Oxygen content");
pub const AorticSinotubularJunctionDimension: Code<'static> = c("79955-1", "Aortic sinotubular junction dimension");
pub const AorticSinotubularJunctionToAorticRootRatio: Code<'static> = c("59116-4", "Aortic Sinotubular Junction to Aortic Root Ratio");
pub const AorticValveAnnulusDiameter: Code<'static> = c("18016-6", "Aortic valve annulus diameter");
pub const AorticValveAreaContinuityVTI: Code<'static> = c("79958-5", "Aortic valve area (Continuity VTI)");
pub const AorticValveAreaContinuityVTIPerBSA: Code<'static> = c("79959-3", "Aortic valve area (Continuity VTI) / BSA");
pub const AorticValveAreaContinuityVmax: Code<'static> = c("79956-9", "Aortic valve area (Continuity Vmax)");
pub const AorticValveAreaContinuityVmaxPerBSA: Code<'static> = c("79957-7", "Aortic valve area (Continuity Vmax) / BSA");
pub const AorticValveCuspSeparation: Code<'static> = c("17996-0", "Aortic Valve Cusp Separation");
pub const AorticValveEffectiveRegurgitantOrificeArea: Code<'static> = c("79960-1", "Aortic valve effective regurgitant orifice area");
pub const AorticValveEffectiveRegurgitantOrificeAreaPISA: Code<'static> = c("77909-0", "Aortic valve Effective regurgitant orifice area (PISA)");
pub const AorticValveEffectiveRegurgitantOrificeAreaVolumetric: Code<'static> = c("77910-8", "Aortic valve Effective regurgitant orifice area (Volumetric)");
pub const AorticValveEjectionTime: Code<'static> = c("18041-4", "Aortic Valve Ejection Time");
pub const AorticValveMeanBloodVelocity: Code<'static> = c("79961-9", "Aortic valve mean blood velocity");
pub const AorticValveMeanGradient: Code<'static> = c("79962-7", "Aortic valve mean gradient");
pub const AorticValveOrificeArea: Code<'static> = c("18089-3", "Aortic valve orifice area");
pub const AorticValveOrificeAreaContinuity: Code<'static> = c("18090-1", "Aortic valve orifice area (Continuity)");
pub const AorticValveOrificeAreaContinuityVTIPlusArea: Code<'static> = c("18091-9", "Aortic valve orifice area (Continuity VTI+Area)");
pub const AorticValveOrificeAreaContinuityVTIPlusDiameter: Code<'static> = c("18092-7", "Aortic valve orifice area (Continuity VTI+Diameter)");
pub const AorticValveOrificeAreaContinuityVmaxPlusArea: Code<'static> = c("18093-5", "Aortic valve orifice area (Continuity Vmax+Area)");
pub const AorticValveOrificeAreaContinuityVmaxPlusDiameter: Code<'static> = c("18094-3", "Aortic valve orifice area (Continuity Vmax+Diameter)");
pub const AorticValvePeakInstantaneousGradient: Code<'static> = c("79963-5", "Aortic valve peak instantaneous gradient");
pub const AorticValvePressureHalfTime: Code<'static> = c("18104-0", "Aortic valve pressure half time");
pub const AorticValveRegurgitantBloodFlowPressureHalfTime: Code<'static> = c("18105-7", "Aortic valve regurgitant blood flow pressure half-time");
pub const AorticValveVTI: Code<'static> = c("79965-0", "Aortic valve VTI");
pub const AorticValveVenaContractaDiameter: Code<'static> = c("77908-2", "Aortic valve vena contracta diameter");
pub const AorticValveVmax: Code<'static> = c("79964-3", "Aortic valve Vmax");
pub const ArterialBloodBaseExcess: Code<'static> = c("1925-7", "Arterial Blood Base Excess");
pub const ArterialBloodCarbonDioxideContent: Code<'static> = c("2026-3", "Arterial Blood Carbon dioxide content");
pub const ArterialBloodCarbonDioxidePartialPressure: Code<'static> = c("2019-8", "Arterial Blood Carbon dioxide partial pressure");
pub const ArterialBloodPH: Code<'static> = c("2744-1", "Arterial Blood pH");
pub const ArterialOxygenContent: Code<'static> = c("19218-7", "Arterial Oxygen content");
pub const ArterialOxygenPartialPressure: Code<'static> = c("2703-7", "Arterial Oxygen partial pressure");
pub const ArterialOxygenPartialPressureAt50PercentSaturation: Code<'static> = c("19214-6", "Arterial Oxygen partial pressure at 50% saturation");
pub const ArterialOxygenSaturation: Code<'static> = c("2708-6", "Arterial Oxygen saturation");
pub const AscendingAortaDimension: Code<'static> = c("79966-8", "Ascending Aorta Dimension");
pub const AscendingAorticDiameter: Code<'static> = c("18012-5", "Ascending Aortic Diameter");
pub const AverageUltrasoundAge: Code<'static> = c("11884-4", "Average Ultrasound Age");
pub const AxTByGAShinozuka1996: Code<'static> = c("33150-4", "AxT by GA, Shinozuka 1996");
pub const AxTShinozuka1996: Code<'static> = c("33078-7", "AxT, Shinozuka 1996");
pub const BCSlope: Code<'static> = c("59126-3", "B-C Slope");
pub const BDJeanty1982: Code<'static> = c("33545-5", "BD, Jeanty 1982");
pub const BPDASUM1989: Code<'static> = c("33079-5", "BPD, ASUM 1989");
pub const BPDAreaCorrected: Code<'static> = c("11824-0", "BPD area corrected");
pub const BPDByGAASUM2000: Code<'static> = c("33151-2", "BPD by GA, ASUM 2000");
pub const BPDByGAHadlock1984: Code<'static> = c("33198-3", "BPD by GA, Hadlock 1984");
pub const BPDByGAJeanty1982: Code<'static> = c("33153-8", "BPD by GA, Jeanty 1982");
pub const BPDByGAMerz1988: Code<'static> = c("33154-6", "BPD by GA, Merz 1988");
pub const BPDByGARempen1991: Code<'static> = c("33155-3", "BPD by GA, Rempen 1991");
pub const BPDByGAShinozuka1996: Code<'static> = c("33156-1", "BPD by GA, Shinozuka 1996");
pub const BPDDoubilet1993: Code<'static> = c("11900-8", "BPD, Doubilet 1993");
pub const BPDHadlock1984: Code<'static> = c("11902-4", "BPD, Hadlock 1984");
pub const BPDHansmann1985: Code<'static> = c("11903-2", "BPD, Hansmann 1985");
pub const BPDHansmann1986: Code<'static> = c("33538-0", "BPD, Hansmann 1986");
pub const BPDJeanty1982: Code<'static> = c("33539-8", "BPD, Jeanty 1982");
pub const BPDJeanty1984: Code<'static> = c("11905-7", "BPD, Jeanty 1984");
pub const BPDKurtz1980: Code<'static> = c("11906-5", "BPD, Kurtz 1980");
pub const BPDLessoway1998: Code<'static> = c("33080-3", "BPD, Lessoway 1998");
pub const BPDMertz1988: Code<'static> = c("33081-1", "BPD, Mertz 1988");
pub const BPDOiChitty1997: Code<'static> = c("33086-0", "BPD-oi, Chitty 1997");
pub const BPDOoChitty1997: Code<'static> = c("33087-8", "BPD-oo, Chitty 1997");
pub const BPDOsaka1989: Code<'static> = c("33082-9", "BPD, Osaka 1989");
pub const BPDOuterInnerByGAChitty1994: Code<'static> = c("33556-2", "BPD outer-inner by GA, Chitty 1994");
pub const BPDOuterOuterByGAChitty1994: Code<'static> = c("33152-0", "BPD outer-outer by GA, Chitty 1994");
pub const BPDRempen1991: Code<'static> = c("33083-7", "BPD, Rempen 1991");
pub const BPDSabbagha1978: Code<'static> = c("11907-3", "BPD, Sabbagha 1978");
pub const BPDShinozuka1996: Code<'static> = c("33084-5", "BPD, Shinozuka 1996");
pub const BPDTokyo1986: Code<'static> = c("33085-2", "BPD, Tokyo 1986");
pub const BPDaHadlock1982: Code<'static> = c("11901-6", "BPDa, Hadlock 1982");
pub const BSA: Code<'static> = c("8277-6", "BSA");
pub const BiophysicalProfileSumScore: Code<'static> = c("11634-3", "Biophysical Profile Sum Score");
pub const BiparietalDiameter: Code<'static> = c("11820-8", "Biparietal Diameter");
pub const BloodBaseExcess: Code<'static> = c("11555-0", "Blood Base Excess");
pub const BloodCarbonDioxideContent: Code<'static> = c("20565-8", "Blood Carbon dioxide content");
pub const BloodCarbonDioxidePartialPressure: Code<'static> = c("11557-6", "Blood Carbon dioxide partial pressure");
pub const BloodOxygenPartialPressure: Code<'static> = c("11556-8", "Blood Oxygen partial pressure");
pub const BloodOxygenSaturation: Code<'static> = c("20564-1", "Blood Oxygen saturation");
pub const BloodPH: Code<'static> = c("11558-4", "Blood pH");
pub const BodySurfaceAreaFormula: Code<'static> = c("8278-4", "Body Surface Area Formula");
pub const BodyTemperature: Code<'static> = c("8310-5", "Body temperature");
pub const BrainMRI: Code<'static> = c("24590-2", "Brain MRI");
pub const BrainPET: Code<'static> = c("44138-6", "Brain PET");
pub const BreastBilateralMammogram: Code<'static> = c("36626-0", "breast - bilateral mammogram");
pub const BreastBilateralMr: Code<'static> = c("30795-9", "breast - bilateral mr");
pub const BreastBilateralMrWContrastIv: Code<'static> = c("36150-1", "breast - bilateral mr w contrast iv");
pub const BreastBilateralMrWoAndWContrastIv: Code<'static> = c("36277-2", "breast - bilateral mr wo and w contrast iv");
pub const BreastFfdMammogram: Code<'static> = c("46342-2", "breast ffd mammogram");
pub const BreastLeftMammogram: Code<'static> = c("36627-8", "breast - left mammogram");
pub const BreastLeftMr: Code<'static> = c("35954-7", "breast - left mr");
pub const BreastLeftMrWContrastIv: Code<'static> = c("36151-9", "breast - left mr w contrast iv");
pub const BreastLeftMrWoAndWContrastIv: Code<'static> = c("36278-0", "breast - left mr wo and w contrast iv");
pub const BreastMrWContrastIv: Code<'static> = c("36149-3", "breast mr w contrast iv");
pub const BreastMrWoAndWContrastIv: Code<'static> = c("36276-4", "breast mr wo and w contrast iv");
pub const BreastRightMammogram: Code<'static> = c("37774-7", "breast - right mammogram");
pub const BreastRightMr: Code<'static> = c("35955-4", "breast - right mr");
pub const BreastRightMrWContrastIv: Code<'static> = c("36152-7", "breast - right mr w contrast iv");
pub const BreastRightMrWoAndWContrastIv: Code<'static> = c("36279-8", "breast - right mr wo and w contrast iv");
pub const BreastUnilateralMammogram: Code<'static> = c("46339-8", "breast - unilateral mammogram");
pub const BreastUnilateralMr: Code<'static> = c("46299-4", "breast - unilateral mr");
pub const BreastUnilateralMrWContrastIv: Code<'static> = c("46323-2", "breast - unilateral mr w contrast iv");
pub const BreastUnilateralMrWoAndWContrastIv: Code<'static> = c("43528-9", "breast - unilateral mr wo and w contrast iv");
pub const BreastUnilateralMrWoContrast: Code<'static> = c("46333-1", "breast - unilateral mr wo contrast");
pub const CEDistance: Code<'static> = c("59122-2", "C-E Distance");
pub const CKDEPIFormulaEstimationOfGFR: Code<'static> = c("62238-1", "CKD-EPI Formula estimation of GFR");
pub const CRLASUM1991: Code<'static> = c("33089-4", "CRL, ASUM 1991");
pub const CRLASUM2000: Code<'static> = c("33090-2", "CRL, ASUM 2000");
pub const CRLByGAASUM2000: Code<'static> = c("33159-5", "CRL by GA ASUM 2000");
pub const CRLByGARempen1991: Code<'static> = c("33160-3", "CRL by GA, Rempen1991");
pub const CRLByGAShinozuka1996: Code<'static> = c("33161-1", "CRL by GA, Shinozuka 1996");
pub const CRLDaya1993: Code<'static> = c("33091-0", "CRL, Daya 1993");
pub const CRLHadlock1992: Code<'static> = c("11910-7", "CRL, Hadlock 1992");
pub const CRLHansmann1985: Code<'static> = c("11911-5", "CRL, Hansmann 1985");
pub const CRLHansmann1986: Code<'static> = c("33540-6", "CRL, Hansmann 1986");
pub const CRLJeanty1982: Code<'static> = c("33092-8", "CRL, Jeanty 1982");
pub const CRLJeanty1984: Code<'static> = c("11917-2", "CRL, Jeanty 1984");
pub const CRLNelson1981: Code<'static> = c("11913-1", "CRL, Nelson 1981");
pub const CRLOsaka1989: Code<'static> = c("33093-6", "CRL, Osaka 1989");
pub const CRLRempen1991: Code<'static> = c("33094-4", "CRL, Rempen 1991");
pub const CRLRobinson1975: Code<'static> = c("11914-9", "CRL, Robinson 1975");
pub const CRLShinozuka1996: Code<'static> = c("33095-1", "CRL, Shinozuka 1996");
pub const CRLTokyo1986: Code<'static> = c("33096-9", "CRL, Tokyo 1986");
pub const CT3DCAMModel: Code<'static> = c("85040-4", "CT 3D CAM model");
pub const CTAbdomenReport: Code<'static> = c("41806-1", "CT Abdomen Report");
pub const CTChestReport: Code<'static> = c("24627-2", "CT Chest Report");
pub const CTPerfusionHeadWithContrastIV: Code<'static> = c("39142-5", "CT perfusion head with contrast IV");
pub const CTUnspecifiedBodyRegion: Code<'static> = c("25045-6", "CT unspecified body region");
pub const CapillaryBloodOxygenSaturation: Code<'static> = c("2709-4", "Capillary Blood Oxygen Saturation");
pub const CapillaryBloodOxygenSaturationByOximetry: Code<'static> = c("2710-2", "Capillary Blood Oxygen Saturation, by Oximetry");
pub const CardiacCatheterizationReport: Code<'static> = c("18745-0", "Cardiac Catheterization Report");
pub const CardiacCircumferenceTransverseByUS: Code<'static> = c("59073-7", "Cardiac Circumference, transverse by US");
pub const CardiacCrossSectionalAreaTransverseByUS: Code<'static> = c("59075-2", "Cardiac Cross-sectional Area, transverse by US");
pub const CardiacElectrophysiologyReport: Code<'static> = c("18750-0", "Cardiac Electrophysiology Report");
pub const CardiacOutputByIndicatorDilution: Code<'static> = c("8737-9", "Cardiac Output by Indicator Dilution");
pub const CardiacRhythm: Code<'static> = c("8884-9", "Cardiac Rhythm");
pub const CardiothoracicAreaRatio: Code<'static> = c("59076-0", "Cardiothoracic Area Ratio");
pub const CardiothoracicCircumferenceRatio: Code<'static> = c("59074-5", "Cardiothoracic Circumference Ratio");
pub const CephalicIndex: Code<'static> = c("11823-2", "Cephalic Index");
pub const CephalicIndexByGAChitty1994: Code<'static> = c("33157-9", "Cephalic Index by GA, Chitty 1994");
pub const CephalicIndexByGAHadlock1981: Code<'static> = c("33158-7", "Cephalic Index by GA, Hadlock 1981");
pub const CervixLength: Code<'static> = c("11961-0", "Cervix Length");
pub const CholesterolInHDL: Code<'static> = c("2085-9", "Cholesterol.in HDL");
pub const CholesterolInLDL: Code<'static> = c("2089-1", "Cholesterol.in LDL");
pub const CisternaMagnaLength: Code<'static> = c("11860-4", "Cisterna Magna Length");
pub const ClavicleLength: Code<'static> = c("11962-8", "Clavicle length");
pub const ClavicleLengthYarkoni1985: Code<'static> = c("33088-6", "Clavicle length, Yarkoni 1985");
pub const ClinicalInformation: Code<'static> = c("55752-0", "Clinical Information");
pub const ClosureToOpeningTime: Code<'static> = c("59082-8", "Closure to Opening Time");
pub const CockroftGaultFormulaEstimationOfGFR: Code<'static> = c("35591-7", "Cockroft-Gault Formula estimation of GFR");
pub const CommunicationOfCriticalResults: Code<'static> = c("73568-8", "Communication of Critical Results");
pub const Complications: Code<'static> = c("55109-3", "Complications");
pub const CompositeUltrasoundAge: Code<'static> = c("11888-5", "Composite Ultrasound Age");
pub const ConceptionDate: Code<'static> = c("33067-0", "Conception Date");
pub const Conclusions: Code<'static> = c("55110-1", "Conclusions");
pub const CrownRumpLength: Code<'static> = c("11957-8", "Crown Rump Length");
pub const CurrentProcedureDescriptions: Code<'static> = c("55111-9", "Current Procedure Descriptions");
pub const DEExcursion: Code<'static> = c("59091-9", "D-E Excursion");
pub const DESlope: Code<'static> = c("59127-1", "D-E Slope");
pub const DEXASkeletalSystemStudyReport: Code<'static> = c("38269-7", "DEXA Skeletal System Study Report");
pub const DPPerDtByUS: Code<'static> = c("59120-6", "dP/dt by US");
pub const DecelerationSlope: Code<'static> = c("20216-8", "Deceleration Slope");
pub const DecelerationTime: Code<'static> = c("20217-6", "Deceleration Time");
pub const DeliveryDateEstimatedFromConceptionDate: Code<'static> = c("53692-0", "Delivery date estimated from conception date");
pub const DeliveryDateEstimatedFromDateFundalHeightReachUmbilicus: Code<'static> = c("57064-8", "Delivery date estimated from date fundal height reach umbilicus");
pub const DeliveryDateEstimatedFromPhysicalExamination: Code<'static> = c("90368-2", "Delivery date estimated from physical examination");
pub const DeliveryDateEstimatedFromPriorGestationalAgeAssessment: Code<'static> = c("53694-6", "Delivery date estimated from prior gestational age assessment");
pub const DeliveryDateEstimatedFromQuickeningDate: Code<'static> = c("57063-0", "Delivery date estimated from quickening date");
pub const DescendingAorticDiameter: Code<'static> = c("18013-3", "Descending Aortic Diameter");
pub const DiagnosticImagingReport: Code<'static> = c("18748-4", "Diagnostic Imaging Report");
pub const DiagnosticInterventionalRadiologyReport: Code<'static> = c("47048-4", "Diagnostic Interventional Radiology Report");
pub const ECGOverallFinding: Code<'static> = c("18810-2", "ECG overall finding");
pub const ECGReport_11524_0: Code<'static> = c("11524-0", "ECG Report");
pub const ECGReport_28010_7: Code<'static> = c("28010-7", "ECG Report");
pub const EDD: Code<'static> = c("11778-8", "EDD");
pub const EDDFromAverageUltrasoundAge: Code<'static> = c("11781-2", "EDD from average ultrasound age");
pub const EDDFromLMP: Code<'static> = c("11779-6", "EDD from LMP");
pub const EDDFromOvulationDate: Code<'static> = c("11780-4", "EDD from ovulation date");
pub const EFSlope: Code<'static> = c("59128-9", "E-F Slope");
pub const EFW1ByShinozuka1996: Code<'static> = c("33141-3", "EFW1 by Shinozuka 1996");
pub const EFW2ByShinozuka1996: Code<'static> = c("33142-1", "EFW2 by Shinozuka 1996");
pub const EFW3ByShinozuka1996: Code<'static> = c("33143-9", "EFW3 by Shinozuka 1996");
pub const EFWByACAndBPDShepard1982: Code<'static> = c("11739-0", "EFW by AC and BPD, Shepard 1982");
pub const EFWByACBPDFLHCHadlock1985: Code<'static> = c("11732-5", "EFW by AC, BPD, FL, HC, Hadlock 1985");
pub const EFWByACBPDFLHadlock1984: Code<'static> = c("11734-1", "EFW by AC, BPD, FL, Hadlock 1984");
pub const EFWByACBPDFLHadlock1985: Code<'static> = c("11735-8", "EFW by AC, BPD, FL, Hadlock 1985");
pub const EFWByACBPDHadlock1984: Code<'static> = c("11738-2", "EFW by AC, BPD, Hadlock 1984");
pub const EFWByACCampbell1975: Code<'static> = c("11756-4", "EFW by AC, Campbell 1975");
pub const EFWByACFLHCHadlock1985: Code<'static> = c("11746-5", "EFW by AC, FL, HC, Hadlock 1985");
pub const EFWByACFLHadlock1984: Code<'static> = c("11750-7", "EFW by AC, FL, Hadlock 1984");
pub const EFWByACFLHadlock1985: Code<'static> = c("11751-5", "EFW by AC, FL, Hadlock 1985");
pub const EFWByACHCHadlock1984: Code<'static> = c("11754-9", "EFW by AC, HC Hadlock 1984");
pub const EFWByBPDAPADTADFLTokyo1987: Code<'static> = c("33144-7", "EFW by BPD, APAD, TAD, FL, Tokyo 1987");
pub const EFWByBPDFTAFLOsaka1990: Code<'static> = c("33140-5", "EFW by BPD, FTA, FL, Osaka 1990");
pub const EFWByBPDTTDHansmann1986: Code<'static> = c("33139-7", "EFW by BPD, TTD, Hansmann 1986");
pub const EFWByGAHadlock1991: Code<'static> = c("33162-9", "EFW by GA, Hadlock 1991");
pub const EFWByGAHansmann1986: Code<'static> = c("33163-7", "EFW by GA, Hansmann 1986");
pub const EFWDiscordance: Code<'static> = c("98031-8", "EFW Discordance");
pub const EFWPercentileRank: Code<'static> = c("11767-1", "EFW percentile rank");
pub const EFWRatio: Code<'static> = c("98032-6", "EFW Ratio");
pub const EVelocityToAnnulusEVelocityRatio: Code<'static> = c("59111-5", "E Velocity to Annulus E Velocity Ratio");
pub const EWavePeakVelocity: Code<'static> = c("59080-2", "E-Wave Peak Velocity");
pub const EchocardiographyReport: Code<'static> = c("42148-7", "Echocardiography Report");
pub const EctopicPregnancies: Code<'static> = c("33065-4", "Ectopic Pregnancies");
pub const EjectionTime: Code<'static> = c("20222-6", "Ejection Time");
pub const EndDiastolicVelocity: Code<'static> = c("11653-3", "End Diastolic Velocity");
pub const EndocardialArea: Code<'static> = c("59094-3", "Endocardial Area");
pub const EndometriumThickness: Code<'static> = c("12145-9", "Endometrium Thickness");
pub const EnvelopeDuration: Code<'static> = c("59108-1", "Envelope Duration");
pub const EpicardialArea: Code<'static> = c("59093-5", "Epicardial Area");
pub const EstimatedLMPByEDD: Code<'static> = c("33066-2", "Estimated LMP by EDD");
pub const EstimatedWeight: Code<'static> = c("11727-5", "Estimated Weight");
pub const ExerciseStressTestReport: Code<'static> = c("18752-6", "Exercise Stress Test Report");
pub const FICKCardiacIndex: Code<'static> = c("8750-2", "FICK Cardiac Index");
pub const FICKCardiacOutput: Code<'static> = c("8736-1", "FICK Cardiac Output");
pub const FLByGAASUM2000: Code<'static> = c("33165-2", "FL by GA, ASUM 2000");
pub const FLByGAChitty1994: Code<'static> = c("33167-8", "FL by GA, Chitty 1994");
pub const FLByGAHadlock1984: Code<'static> = c("33166-0", "FL by GA, Hadlock 1984");
pub const FLByGAJeanty1982: Code<'static> = c("33168-6", "FL by GA, Jeanty 1982");
pub const FLByGAMerz1988: Code<'static> = c("33169-4", "FL by GA, Merz 1988");
pub const FLByGAShinozuka1996: Code<'static> = c("33170-2", "FL by GA, Shinozuka 1996");
pub const FLChitty1997: Code<'static> = c("33098-5", "FL, Chitty 1997");
pub const FLHadlock1984: Code<'static> = c("11920-6", "FL, Hadlock 1984");
pub const FLHansmann1985: Code<'static> = c("11921-4", "FL, Hansmann 1985");
pub const FLHansmann1986: Code<'static> = c("33541-4", "FL, Hansmann 1986");
pub const FLHohler1982: Code<'static> = c("11922-2", "FL, Hohler 1982");
pub const FLJeanty1982: Code<'static> = c("33099-3", "FL, Jeanty 1982");
pub const FLJeanty1984: Code<'static> = c("11923-0", "FL, Jeanty 1984");
pub const FLLessoway1998: Code<'static> = c("33100-9", "FL, Lessoway 1998");
pub const FLMerz1987: Code<'static> = c("11924-8", "FL, Merz 1987");
pub const FLMerz1988: Code<'static> = c("33542-2", "FL, Merz 1988");
pub const FLOsaka1989: Code<'static> = c("33101-7", "FL, Osaka 1989");
pub const FLPerAC: Code<'static> = c("11871-1", "FL/AC");
pub const FLPerBPD: Code<'static> = c("11872-9", "FL/BPD");
pub const FLPerHC: Code<'static> = c("11873-7", "FL/HC");
pub const FLShinozuka1996: Code<'static> = c("33102-5", "FL, Shinozuka 1996");
pub const FLTokyo1986: Code<'static> = c("33103-3", "FL, Tokyo 1986");
pub const FWPByGAAlexander1996: Code<'static> = c("33185-0", "FWP by GA, Alexander, 1996");
pub const FWPByGABrenner1976: Code<'static> = c("33189-2", "FWP by GA, Brenner 1976");
pub const FWPByGAHadlock1985: Code<'static> = c("33190-0", "FWP by GA, Hadlock 1985");
pub const FWPByGAWilliams1982: Code<'static> = c("33184-3", "FWP by GA, Williams, 1982");
pub const FemaleSingletonBWPByGAArbuckle1993: Code<'static> = c("33187-6", "Female Singleton BWP by GA, Arbuckle 1993");
pub const FemaleTwinsBWPByGAArbuckle1993: Code<'static> = c("33188-4", "Female Twins BWP by GA, Arbuckle 1993");
pub const FemurLength: Code<'static> = c("11963-6", "Femur Length");
pub const FetalACAResistivityIndex: Code<'static> = c("12012-1", "Fetal ACA Resistivity Index");
pub const FetalBreathing: Code<'static> = c("11632-7", "Fetal Breathing");
pub const FetalHeartPosition: Code<'static> = c("11992-5", "Fetal Heart Position");
pub const FetalHeartRate: Code<'static> = c("11948-7", "Fetal Heart Rate");
pub const FetalHeartReactivity: Code<'static> = c("11633-5", "Fetal Heart Reactivity");
pub const FetalMCAResistivityIndex: Code<'static> = c("12014-7", "Fetal MCA Resistivity Index");
pub const FetalTone: Code<'static> = c("11635-0", "Fetal Tone");
pub const FetalTrunkCrossSectionalAreaOsaka1989: Code<'static> = c("33138-9", "Fetal Trunk Cross-Sectional Area, Osaka 1989");
pub const FetusID: Code<'static> = c("11951-1", "Fetus ID");
pub const FibulaByGAJeanty1983: Code<'static> = c("33164-5", "Fibula by GA, Jeanty 1983");
pub const FibulaJeanty1983: Code<'static> = c("33097-7", "Fibula, Jeanty 1983");
pub const FibulaLength: Code<'static> = c("11964-4", "Fibula length");
pub const FibulaMerz1987: Code<'static> = c("11918-0", "Fibula, Merz 1987");
pub const FillingTime: Code<'static> = c("59119-8", "Filling Time");
pub const Findings: Code<'static> = c("59776-5", "Findings");
pub const FirstQuadrantDiameter: Code<'static> = c("11624-4", "First Quadrant Diameter");
pub const FlowArea: Code<'static> = c("20226-7", "Flow Area");
pub const FlowRadius: Code<'static> = c("59102-4", "Flow Radius");
pub const FluoroscopyStudyReport: Code<'static> = c("49512-7", "Fluoroscopy Study Report");
pub const FollicleDiameter: Code<'static> = c("11793-7", "Follicle Diameter");
pub const FootLength: Code<'static> = c("11965-1", "Foot length");
pub const FootLengthMercer1987: Code<'static> = c("11926-3", "Foot Length, Mercer 1987");
pub const ForamenOvaleDiameterPerAorticRootDiameter: Code<'static> = c("59077-8", "Foramen Ovale Diameter/Aortic Root Diameter");
pub const FourthQuadrantDiameter: Code<'static> = c("11623-6", "Fourth Quadrant Diameter");
pub const FractionalShortening: Code<'static> = c("59132-1", "Fractional Shortening");
pub const GSByGARempen1991: Code<'static> = c("33171-0", "GS by GA, Rempen 1991");
pub const GSDaya1991: Code<'static> = c("33104-1", "GS, Daya 1991");
pub const GSHansmann1979: Code<'static> = c("33105-8", "GS, Hansmann 1979");
pub const GSHansmann1982: Code<'static> = c("33106-6", "GS, Hansmann 1982");
pub const GSHellman1969: Code<'static> = c("11928-9", "GS, Hellman 1969");
pub const GSNyberg1992: Code<'static> = c("33107-4", "GS, Nyberg 1992");
pub const GSRempen1991: Code<'static> = c("11929-7", "GS, Rempen 1991");
pub const GSTokyo1986: Code<'static> = c("33108-2", "GS, Tokyo 1986");
pub const GestationalAge: Code<'static> = c("18185-9", "Gestational Age");
pub const GestationalAgeByLMP: Code<'static> = c("11885-1", "Gestational Age by LMP");
pub const GestationalAgeByOvulationDate: Code<'static> = c("11886-9", "Gestational Age by ovulation date");
pub const GestationalSacDiameter: Code<'static> = c("11850-5", "Gestational Sac Diameter");
pub const GlomerularFiltrationRateBlackMDRD: Code<'static> = c("48643-1", "Glomerular Filtration Rate black (MDRD)");
pub const GlomerularFiltrationRateCreatinineBasedFormulaSchwartz: Code<'static> = c("50384-7", "Glomerular Filtration Rate Creatinine-based formula (Schwartz)");
pub const GlomerularFiltrationRateCystatinBasedFormula: Code<'static> = c("50210-4", "Glomerular Filtration Rate Cystatin-based formula");
pub const GlomerularFiltrationRateFemaleMDRD: Code<'static> = c("50044-7", "Glomerular Filtration Rate female (MDRD)");
pub const GlomerularFiltrationRateMDRD: Code<'static> = c("33914-3", "Glomerular Filtration Rate (MDRD)");
pub const GlomerularFiltrationRateNonBlackMDRD: Code<'static> = c("48642-3", "Glomerular Filtration Rate non-black (MDRD)");
pub const Glucose: Code<'static> = c("14749-6", "Glucose");
pub const Gravida: Code<'static> = c("11996-6", "Gravida");
pub const GrossBodyMovement: Code<'static> = c("11631-9", "Gross Body Movement");
pub const HCASUM2000: Code<'static> = c("33109-0", "HC, ASUM 2000");
pub const HCByGAASUM2000: Code<'static> = c("33172-8", "HC by GA, ASUM 2000");
pub const HCByGAHadlock1984: Code<'static> = c("33173-6", "HC by GA, Hadlock 1984");
pub const HCByGAJeanty1982: Code<'static> = c("33175-1", "HC by GA, Jeanty 1982");
pub const HCByGAMerz1988: Code<'static> = c("33176-9", "HC by GA, Merz 1988");
pub const HCDerivedByGAChitty1994: Code<'static> = c("33174-4", "HC derived by GA, Chitty 1994");
pub const HCDerivedChitty1997: Code<'static> = c("33111-6", "HC derived, Chitty 1997");
pub const HCHadlock1984: Code<'static> = c("11932-1", "HC, Hadlock 1984");
pub const HCHansmann1985: Code<'static> = c("33112-4", "HC, Hansmann 1985");
pub const HCHansmann1986: Code<'static> = c("33543-0", "HC, Hansmann 1986");
pub const HCJeanty1982: Code<'static> = c("33113-2", "HC, Jeanty 1982");
pub const HCJeanty1984: Code<'static> = c("11934-7", "HC, Jeanty 1984");
pub const HCLessoway1998: Code<'static> = c("33114-0", "HC, Lessoway 1998");
pub const HCMeasuredChitty1997: Code<'static> = c("33110-8", "HC measured, Chitty 1997");
pub const HCMerz1988: Code<'static> = c("33115-7", "HC Merz, 1988");
pub const HCPerAC: Code<'static> = c("11947-9", "HC/AC");
pub const HCPerACByGACampbell1977: Code<'static> = c("80416-1", "HC/AC by GA, Campbell 1977");
pub const HER2: Code<'static> = c("48676-1", "HER2");
pub const HRCorrectedMeanVelocityOfCircumferentialFiberShortening: Code<'static> = c("59118-0", "HR-Corrected Mean Velocity of Circumferential Fiber Shortening");
pub const HePerHimPerHisPerHisPerHimself: Code<'static> = c("LA29518-0", "he/him/his/his/himself");
pub const HeadCT: Code<'static> = c("24725-4", "Head CT");
pub const HeadCircumference: Code<'static> = c("11984-2", "Head Circumference");
pub const HealthStatus: Code<'static> = c("11323-3", "Health status");
pub const HeartRate: Code<'static> = c("8867-4", "Heart rate");
pub const HeartRateCorrectedEjectionTime: Code<'static> = c("59086-9", "Heart Rate-Corrected Ejection Time");
pub const HeartRateCorrectedPreEjectionPeriod: Code<'static> = c("59087-7", "Heart Rate-Corrected Pre-Ejection Period");
pub const Hematocrit: Code<'static> = c("20570-8", "Hematocrit");
pub const Hemoglobin: Code<'static> = c("718-7", "Hemoglobin");
pub const HepaticVeinAtrialContractionReversalPeakVelocity: Code<'static> = c("29474-4", "Hepatic Vein Atrial Contraction Reversal Peak Velocity");
pub const HepaticVeinDiastolicPeakVelocity: Code<'static> = c("29472-8", "Hepatic Vein Diastolic Peak Velocity");
pub const HepaticVeinSystolicPeakVelocity: Code<'static> = c("29471-0", "Hepatic Vein Systolic Peak Velocity");
pub const HepaticVeinSystolicToDiastolicRatio: Code<'static> = c("29473-6", "Hepatic Vein Systolic to Diastolic Ratio");
pub const History: Code<'static> = c("11329-0", "History");
pub const HistoryOfFamilyMemberDiseases: Code<'static> = c("10157-6", "History of Family Member Diseases");
pub const HistoryOfMedicalDeviceUse: Code<'static> = c("46264-8", "History of medical device use");
pub const HistoryOfMedicationUse: Code<'static> = c("10160-0", "History Of Medication Use");
pub const HolterStudyReport: Code<'static> = c("18754-2", "Holter Study Report");
pub const HumerusJeanty1984: Code<'static> = c("11936-2", "Humerus, Jeanty 1984");
pub const HumerusLength: Code<'static> = c("11966-9", "Humerus length");
pub const HumerusLengthASUM2000: Code<'static> = c("33116-5", "Humerus Length, ASUM 2000");
pub const HumerusLengthByGAASUM2000: Code<'static> = c("33177-7", "Humerus Length by GA, ASUM 2000");
pub const HumerusLengthOsaka1989: Code<'static> = c("33117-3", "Humerus Length, Osaka 1989");
pub const HumerusMerz1987: Code<'static> = c("11937-0", "Humerus, Merz 1987");
pub const ICAPerCCAVelocityRatio: Code<'static> = c("33868-1", "ICA/CCA velocity ratio");
pub const IODMayden1982: Code<'static> = c("33122-3", "IOD, Mayden 1982");
pub const IODTrout1994: Code<'static> = c("33123-1", "IOD, Trout 1994");
pub const Impressions: Code<'static> = c("19005-8", "Impressions");
pub const IndicationsForProcedure: Code<'static> = c("18785-6", "Indications for Procedure");
pub const InferiorVenaCavaDiameter_18006_7: Code<'static> = c("18006-7", "Inferior Vena Cava Diameter");
pub const InferiorVenaCavaDiameter_79967_6: Code<'static> = c("79967-6", "Inferior vena cava diameter");
pub const InferiorVenaCavaPercentCollapse: Code<'static> = c("18050-5", "Inferior Vena Cava % Collapse");
pub const InnerOrbitalDiameter: Code<'static> = c("33070-4", "Inner Orbital Diameter");
pub const InterventricularSeptumDiastolicDimension2D: Code<'static> = c("79969-2", "Interventricular septum diastolic dimension 2D");
pub const InterventricularSeptumDiastolicDimensionMM: Code<'static> = c("79968-4", "Interventricular septum diastolic dimension MM");
pub const InterventricularSeptumDiastolicThickness: Code<'static> = c("18154-5", "Interventricular Septum Diastolic Thickness");
pub const InterventricularSeptumPercentThickening: Code<'static> = c("18054-7", "Interventricular Septum % Thickening");
pub const InterventricularSeptumSystolicDimension2D: Code<'static> = c("79971-8", "Interventricular septum systolic dimension 2D");
pub const InterventricularSeptumSystolicDimensionMM: Code<'static> = c("79970-0", "Interventricular septum systolic dimension MM");
pub const InterventricularSeptumSystolicThickness: Code<'static> = c("18158-6", "Interventricular Septum Systolic Thickness");
pub const InterventricularSeptumTimeToPeakDisplacement: Code<'static> = c("79972-6", "Interventricular septum time to peak displacement");
pub const InterventricularSeptumToPosteriorWallThicknessRatio: Code<'static> = c("18155-2", "Interventricular Septum to Posterior Wall Thickness Ratio");
pub const IntravascularArterialMeanPressure: Code<'static> = c("8478-0", "Intravascular arterial mean pressure");
pub const IntravascularDiastolicBloodPressure: Code<'static> = c("8462-4", "Intravascular diastolic blood pressure");
pub const IntravascularSystolicBloodPressure: Code<'static> = c("8480-6", "Intravascular Systolic Blood pressure");
pub const IsovolumicContractionTime: Code<'static> = c("59084-4", "Isovolumic Contraction Time");
pub const IsovolumicRelaxationTime: Code<'static> = c("59083-6", "Isovolumic Relaxation Time");
pub const JetArea: Code<'static> = c("59123-0", "Jet Area");
pub const KeyImages: Code<'static> = c("55113-5", "Key Images");
pub const LMP: Code<'static> = c("8665-2", "LMP");
pub const LVBasalAnteriorTimeToSVmaxTsBasalAnterior: Code<'static> = c("80036-7", "LV basal anterior time to S Vmax (Ts-basal anterior)");
pub const LVBasalAnteroseptalTimeToSVmaxTSBasalAnteroseptal: Code<'static> = c("80037-5", "LV basal anteroseptal time to S Vmax (TS-basal anteroseptal)");
pub const LVBasalInferiorTimeToSVmaxTsBasalInferior: Code<'static> = c("80038-3", "LV basal inferior time to S Vmax (Ts-basal inferior)");
pub const LVBasalLateralTimeToSVmaxTsBasalLateral: Code<'static> = c("80039-1", "LV basal lateral time to S Vmax (Ts-basal lateral)");
pub const LVBasalPosteriorTimeToSVmaxTsBasalPosterior: Code<'static> = c("80040-9", "LV basal posterior time to S Vmax (Ts-basal posterior)");
pub const LVBasalSeptalTimeToSVmaxTsBasalSeptal: Code<'static> = c("80041-7", "LV basal septal time to S Vmax (Ts-basal septal)");
pub const LVEjectionFraction: Code<'static> = c("10230-1", "LV Ejection Fraction");
pub const LVMidAnteriorTimeToSVmaxTsMidAnterior: Code<'static> = c("80042-5", "LV mid anterior time to S Vmax (Ts-mid anterior)");
pub const LVMidAnteroseptalTimeToSVmaxTsMidAnteroseptal: Code<'static> = c("80043-3", "LV mid anteroseptal time to S Vmax (Ts-mid anteroseptal)");
pub const LVMidInferiorTimeToSVmaxTsMidInferior: Code<'static> = c("80044-1", "LV mid inferior time to S Vmax (Ts-mid inferior)");
pub const LVMidLateralTimeToSVmaxTsMidLateral: Code<'static> = c("80045-8", "LV mid lateral time to S Vmax (Ts-mid lateral)");
pub const LVMidPosteriorTimeToSVmaxTsMidPosterior: Code<'static> = c("80046-6", "LV mid posterior time to S Vmax (Ts-mid posterior)");
pub const LVMidSeptalTimeToSVmaxTsMidSeptal: Code<'static> = c("80047-4", "LV mid septal time to S Vmax (Ts-mid septal)");
pub const LVStrokeVolume: Code<'static> = c("8769-2", "LV Stroke Volume");
pub const LVTsSDDyssynchronyIndex: Code<'static> = c("80048-2", "LV Ts-SD (Dyssynchrony Index)");
pub const LVWallMotionSegmentalFindings: Code<'static> = c("18118-0", "LV Wall Motion Segmental Findings");
pub const LateralVentricleWidth: Code<'static> = c("12171-5", "Lateral Ventricle width");
pub const LeafletSeparation: Code<'static> = c("59109-9", "Leaflet Separation");
pub const LeafletThickness: Code<'static> = c("59110-7", "Leaflet Thickness");
pub const LeftAtrialAppendagePeakVelocity: Code<'static> = c("29486-8", "Left Atrial Appendage Peak Velocity");
pub const LeftAtrialEndSystolicArea2C: Code<'static> = c("79973-4", "Left atrial end systolic area 2C");
pub const LeftAtrialEndSystolicArea4C: Code<'static> = c("79974-2", "Left atrial end systolic area 4C");
pub const LeftAtrialEndSystolicDiameterAP2D: Code<'static> = c("79975-9", "Left atrial end systolic diameter (AP) 2D");
pub const LeftAtrialEndSystolicDiameterAP2DPerBSA: Code<'static> = c("79976-7", "Left atrial end systolic diameter (AP) 2D / BSA");
pub const LeftAtrialEndSystolicDiameterAPMM: Code<'static> = c("79977-5", "Left atrial end systolic diameter (AP) MM");
pub const LeftAtrialEndSystolicDiameterAPMMPerBSA: Code<'static> = c("79978-3", "Left atrial end systolic diameter (AP) MM / BSA");
pub const LeftAtrialEndSystolicLength2C: Code<'static> = c("79979-1", "Left atrial end systolic length 2C");
pub const LeftAtrialEndSystolicLength4C: Code<'static> = c("79980-9", "Left atrial end systolic length 4C");
pub const LeftAtrialEndSystolicVolumeBiplaneAreaLength: Code<'static> = c("79981-7", "Left atrial end systolic volume biplane (area-length)");
pub const LeftAtrialEndSystolicVolumeBiplaneAreaLengthPerBSA: Code<'static> = c("79982-5", "Left atrial end systolic volume biplane (area-length) / BSA");
pub const LeftAtrialEndSystolicVolumeBiplaneMOD: Code<'static> = c("79983-3", "Left atrial end systolic volume biplane (MOD)");
pub const LeftAtrialEndSystolicVolumeBiplaneMODPerBSA: Code<'static> = c("79984-1", "Left atrial end systolic volume biplane (MOD) / BSA");
pub const LeftAtrialEndSystolicVolumeSinglePlane2CMOD: Code<'static> = c("79985-8", "Left atrial end systolic volume single plane 2C (MOD)");
pub const LeftAtrialEndSystolicVolumeSinglePlane4CMOD: Code<'static> = c("79986-6", "Left atrial end systolic volume single plane 4C (MOD)");
pub const LeftAtriumAnteroPosteriorSystolicDimension: Code<'static> = c("29469-4", "Left Atrium Antero-posterior Systolic Dimension");
pub const LeftAtriumAreaA4CView: Code<'static> = c("17977-0", "Left Atrium Area A4C view");
pub const LeftAtriumToAorticRootRatio: Code<'static> = c("17985-3", "Left Atrium to Aortic Root Ratio");
pub const LeftAtriumVolumeToRightAtriumVolumeRatio: Code<'static> = c("59131-3", "Left Atrium Volume to Right Atrium Volume Ratio");
pub const LeftFetalEarLength: Code<'static> = c("53684-7", "Left Fetal Ear Length");
pub const LeftKidneyLength: Code<'static> = c("11834-9", "Left Kidney length");
pub const LeftKidneyThickness: Code<'static> = c("11853-9", "Left Kidney thickness");
pub const LeftKidneyWidth: Code<'static> = c("11825-7", "Left Kidney width");
pub const LeftOvaryHeight: Code<'static> = c("11857-0", "Left Ovary Height");
pub const LeftOvaryLength: Code<'static> = c("11840-6", "Left Ovary Length");
pub const LeftOvaryVolume: Code<'static> = c("12164-0", "Left Ovary Volume");
pub const LeftOvaryWidth: Code<'static> = c("11829-9", "Left Ovary Width");
pub const LeftPulmonaryArteryDiameter_18019_0: Code<'static> = c("18019-0", "Left Pulmonary Artery Diameter");
pub const LeftPulmonaryArteryDiameter_79987_4: Code<'static> = c("79987-4", "Left pulmonary artery diameter");
pub const LeftVenticularPosteriorWallTimeToPeakDisplacement: Code<'static> = c("79988-2", "Left venticular posterior wall time to peak displacement");
pub const LeftVentricleCardiacOutput: Code<'static> = c("8735-3", "Left Ventricle Cardiac Output");
pub const LeftVentricleDiastolicMajorAxis: Code<'static> = c("18077-8", "Left Ventricle diastolic major axis");
pub const LeftVentricleEToATissueVelocityRatio: Code<'static> = c("59129-7", "Left Ventricle E to A Tissue Velocity Ratio");
pub const LeftVentricleInternalEndDiastolicDimension: Code<'static> = c("29436-3", "Left Ventricle Internal End Diastolic Dimension");
pub const LeftVentricleInternalSystolicDimension: Code<'static> = c("29438-9", "Left Ventricle Internal Systolic Dimension");
pub const LeftVentricleMass: Code<'static> = c("18087-7", "Left Ventricle Mass");
pub const LeftVentricleMeridionalWallStress: Code<'static> = c("59097-6", "Left Ventricle Meridional Wall Stress");
pub const LeftVentriclePerRightVentricleDiameterRatio: Code<'static> = c("59078-6", "Left Ventricle/Right Ventricle Diameter Ratio");
pub const LeftVentriclePosteriorWallDiastolicThickness: Code<'static> = c("18152-9", "Left Ventricle Posterior Wall Diastolic Thickness");
pub const LeftVentriclePosteriorWallPercentThickening: Code<'static> = c("18053-9", "Left Ventricle Posterior Wall % Thickening");
pub const LeftVentriclePosteriorWallSystolicThickness: Code<'static> = c("18156-0", "Left Ventricle Posterior Wall Systolic Thickness");
pub const LeftVentricleSystolicMajorAxis: Code<'static> = c("18076-0", "Left Ventricle systolic major axis");
pub const LeftVentricularCardiacOutput: Code<'static> = c("24526-6", "Left ventricular cardiac output");
pub const LeftVentricularCardiacOutput2CAreaLength: Code<'static> = c("93647-6", "Left ventricular cardiac output 2C (area-length )");
pub const LeftVentricularCardiacOutput2CMOD: Code<'static> = c("93650-0", "Left ventricular cardiac output 2C (MOD)");
pub const LeftVentricularCardiacOutput3D: Code<'static> = c("81390-7", "Left ventricular cardiac output 3D");
pub const LeftVentricularCardiacOutput4CAreaLength: Code<'static> = c("93648-4", "Left ventricular cardiac output 4C (area-length)");
pub const LeftVentricularCardiacOutput4CMOD: Code<'static> = c("93651-8", "Left ventricular cardiac output 4C (MOD)");
pub const LeftVentricularCardiacOutputBiplaneAreaLength: Code<'static> = c("93649-2", "Left ventricular cardiac output (biplane area-length)");
pub const LeftVentricularCardiacOutputBiplaneEllipse: Code<'static> = c("20204-4", "Left ventricular cardiac output (biplane ellipse)");
pub const LeftVentricularCardiacOutputBullet: Code<'static> = c("20205-1", "Left ventricular cardiac output (bullet)");
pub const LeftVentricularCardiacOutputCalculated: Code<'static> = c("76565-1", "Left ventricular cardiac output (calculated)");
pub const LeftVentricularCardiacOutputCube: Code<'static> = c("76567-7", "Left ventricular cardiac output (cube)");
pub const LeftVentricularCardiacOutputCubed: Code<'static> = c("20206-9", "Left ventricular cardiac output (cubed)");
pub const LeftVentricularCardiacOutputGibson: Code<'static> = c("76571-9", "Left ventricular cardiac output (Gibson)");
pub const LeftVentricularCardiacOutputLVOT: Code<'static> = c("20207-7", "Left ventricular cardiac output (LVOT)");
pub const LeftVentricularCardiacOutputMModeCalculated: Code<'static> = c("76564-4", "Left ventricular cardiac output M-mode (calculated)");
pub const LeftVentricularCardiacOutputMModeCube: Code<'static> = c("76566-9", "Left ventricular cardiac output M-mode (cube)");
pub const LeftVentricularCardiacOutputMModeGibson: Code<'static> = c("76570-1", "Left ventricular cardiac output M-mode (Gibson)");
pub const LeftVentricularCardiacOutputMModeTeichholz: Code<'static> = c("76568-5", "Left ventricular cardiac output M-mode (Teichholz)");
pub const LeftVentricularCardiacOutputModifiedBiplane: Code<'static> = c("20208-5", "Left ventricular cardiac output (modified biplane)");
pub const LeftVentricularCardiacOutputSinglePlaneEllipse: Code<'static> = c("20212-7", "Left ventricular cardiac output (single plane ellipse)");
pub const LeftVentricularCardiacOutputTeichholz: Code<'static> = c("76569-3", "Left ventricular cardiac output (Teichholz)");
pub const LeftVentricularEDVolume: Code<'static> = c("8821-1", "Left Ventricular ED Volume");
pub const LeftVentricularESVolume: Code<'static> = c("8823-7", "Left Ventricular ES Volume");
pub const LeftVentricularEjectionFraction3D: Code<'static> = c("79990-8", "Left ventricular ejection fraction 3D");
pub const LeftVentricularEjectionFractionBiplaneMOD: Code<'static> = c("79991-6", "Left ventricular ejection fraction biplane (MOD)");
pub const LeftVentricularEjectionFractionByAngiography: Code<'static> = c("8808-8", "Left Ventricular Ejection Fraction by Angiography");
pub const LeftVentricularEjectionFractionByCT: Code<'static> = c("8810-4", "Left ventricular ejection fraction by CT");
pub const LeftVentricularEjectionFractionByMR: Code<'static> = c("8811-2", "Left ventricular ejection fraction by MR");
pub const LeftVentricularEjectionFractionByUS: Code<'static> = c("18043-0", "Left Ventricular Ejection Fraction by US");
pub const LeftVentricularEjectionFractionSinglePlane2CMOD: Code<'static> = c("79992-4", "Left ventricular ejection fraction single plane 2C (MOD)");
pub const LeftVentricularEjectionFractionSinglePlane4CMOD: Code<'static> = c("79993-2", "Left ventricular ejection fraction single plane 4C (MOD)");
pub const LeftVentricularEjectionFractionTeichholz2D: Code<'static> = c("77891-0", "Left ventricular ejection fraction (Teichholz) 2D");
pub const LeftVentricularEjectionFractionTeichholzMM: Code<'static> = c("18049-7", "Left ventricular ejection fraction (Teichholz) MM");
pub const LeftVentricularEndDiastolicLength4C: Code<'static> = c("79994-0", "Left ventricular end diastolic length 4C");
pub const LeftVentricularEndDiastolicVolume: Code<'static> = c("18026-5", "Left Ventricular End Diastolic Volume");
pub const LeftVentricularEndDiastolicVolume3D: Code<'static> = c("79995-7", "Left ventricular end diastolic volume (3D)");
pub const LeftVentricularEndDiastolicVolumeBiplaneMOD: Code<'static> = c("79996-5", "Left ventricular end diastolic volume biplane (MOD)");
pub const LeftVentricularEndDiastolicVolumeBiplaneMODPerBSA: Code<'static> = c("79997-3", "Left ventricular end diastolic volume biplane (MOD) / BSA");
pub const LeftVentricularEndDiastolicVolumeSinglePlane2CMOD: Code<'static> = c("79998-1", "Left ventricular end diastolic volume single plane 2C (MOD)");
pub const LeftVentricularEndDiastolicVolumeSinglePlane4CMOD: Code<'static> = c("79999-9", "Left ventricular end diastolic volume single plane 4C (MOD)");
pub const LeftVentricularEndSystolicVolume: Code<'static> = c("18148-7", "Left Ventricular End Systolic Volume");
pub const LeftVentricularEndSystolicVolume3D: Code<'static> = c("80000-3", "Left ventricular end systolic volume (3D)");
pub const LeftVentricularEndSystolicVolumeBiplaneMOD: Code<'static> = c("80001-1", "Left ventricular end systolic volume biplane (MOD)");
pub const LeftVentricularEndSystolicVolumeBiplaneMODPerBSA: Code<'static> = c("80002-9", "Left ventricular end systolic volume biplane (MOD) / BSA");
pub const LeftVentricularEndSystolicVolumeSinglePlane2CMOD: Code<'static> = c("80003-7", "Left ventricular end systolic volume single plane 2C (MOD)");
pub const LeftVentricularEndSystolicVolumeSinglePlane4CMOD: Code<'static> = c("80004-5", "Left ventricular end systolic volume single plane 4C (MOD)");
pub const LeftVentricularEndocardialAreaSAXPMLevel: Code<'static> = c("80005-2", "Left ventricular endocardial area SAX PM level");
pub const LeftVentricularEpicardialAreaSAXPMLevel: Code<'static> = c("80006-0", "Left ventricular epicardial area SAX PM level");
pub const LeftVentricularFractionalShortening: Code<'static> = c("18051-3", "Left Ventricular Fractional Shortening");
pub const LeftVentricularFractionalShorteningOfMinorAxis2D: Code<'static> = c("29434-8", "Left ventricular fractional shortening (of minor axis) (2D)");
pub const LeftVentricularFractionalShorteningOfMinorAxisMM: Code<'static> = c("29435-5", "Left ventricular fractional shortening (of minor axis) (MM)");
pub const LeftVentricularInternalDiastolicDimension2D: Code<'static> = c("80007-8", "Left ventricular internal diastolic dimension - 2D");
pub const LeftVentricularInternalDiastolicDimensionMM: Code<'static> = c("80008-6", "Left ventricular internal diastolic dimension - MM");
pub const LeftVentricularInternalDiastolicDimensionPerBSA_80009_4: Code<'static> = c("80009-4", "Left ventricular internal diastolic dimension / BSA");
pub const LeftVentricularInternalDiastolicDimensionPerBSA_80010_2: Code<'static> = c("80010-2", "Left ventricular internal diastolic dimension / BSA");
pub const LeftVentricularInternalSystolicDimension2D: Code<'static> = c("80011-0", "Left ventricular internal systolic dimension - 2D");
pub const LeftVentricularInternalSystolicDimensionMM: Code<'static> = c("80012-8", "Left ventricular internal systolic dimension - MM");
pub const LeftVentricularInternalSystolicDimensionPerBSA_80013_6: Code<'static> = c("80013-6", "Left ventricular internal systolic dimension / BSA");
pub const LeftVentricularInternalSystolicDimensionPerBSA_80014_4: Code<'static> = c("80014-4", "Left ventricular internal systolic dimension / BSA");
pub const LeftVentricularIsovolumicRelaxationTime: Code<'static> = c("18071-1", "Left Ventricular Isovolumic Relaxation Time");
pub const LeftVentricularIsovolumicRelaxationTimeByTDI: Code<'static> = c("80015-1", "Left ventricular isovolumic relaxation time by TDI");
pub const LeftVentricularMassAreaLength: Code<'static> = c("80016-9", "Left ventricular mass (area-length)");
pub const LeftVentricularMassAreaLengthPerBSA: Code<'static> = c("80017-7", "Left ventricular mass (area-length) / BSA");
pub const LeftVentricularMassAreaLengthPerHeight27: Code<'static> = c("80018-5", "Left ventricular mass (area-length) / height^^2.7");
pub const LeftVentricularMassDimensionMethod2D: Code<'static> = c("80019-3", "Left ventricular mass (dimension method) 2D");
pub const LeftVentricularMassDimensionMethod2DPerBSA: Code<'static> = c("80020-1", "Left ventricular mass (dimension method) 2D / BSA");
pub const LeftVentricularMassDimensionMethod2DPerHeight27: Code<'static> = c("80021-9", "Left ventricular mass (dimension method) 2D / height^^2.7");
pub const LeftVentricularMassDimensionMethodMM: Code<'static> = c("80022-7", "Left ventricular mass (dimension method) MM");
pub const LeftVentricularMassDimensionMethodMMPerBSA: Code<'static> = c("80023-5", "Left ventricular mass (dimension method) MM / BSA");
pub const LeftVentricularMassDimensionMethodMMPerHeight27: Code<'static> = c("80024-3", "Left ventricular mass (dimension method) MM / height^^2.7");
pub const LeftVentricularMassTruncatedEllipse: Code<'static> = c("80025-0", "Left ventricular mass (truncated ellipse)");
pub const LeftVentricularMassTruncatedEllipsePerBSA: Code<'static> = c("80026-8", "Left ventricular mass (truncated ellipse) / BSA");
pub const LeftVentricularMassTruncatedEllipsePerHeight27: Code<'static> = c("80027-6", "Left ventricular mass (truncated ellipse) / height^^2.7");
pub const LeftVentricularOutflowTractDimension2D: Code<'static> = c("80028-4", "Left ventricular outflow tract dimension (2D)");
pub const LeftVentricularOutflowTractPerAorticValveVTI: Code<'static> = c("93632-8", "Left ventricular outflow tract/Aortic valve VTI");
pub const LeftVentricularOutflowTractVTI: Code<'static> = c("80030-0", "Left ventricular outflow tract VTI");
pub const LeftVentricularOutflowTractVmax: Code<'static> = c("80029-2", "Left ventricular outflow tract Vmax");
pub const LeftVentricularPosteriorWallDiastolicThickness_80031_8: Code<'static> = c("80031-8", "Left ventricular posterior wall diastolic thickness");
pub const LeftVentricularPosteriorWallDiastolicThickness_80032_6: Code<'static> = c("80032-6", "Left ventricular posterior wall diastolic thickness");
pub const LeftVentricularPosteriorWallSystolicThickness_80033_4: Code<'static> = c("80033-4", "Left ventricular posterior wall systolic thickness");
pub const LeftVentricularPosteriorWallSystolicThickness_80034_2: Code<'static> = c("80034-2", "Left ventricular posterior wall systolic thickness");
pub const LeftVentricularPreEjectionPeriod: Code<'static> = c("79989-0", "Left ventricular pre-ejection period");
pub const LeftVentricularSphericityIndexEndDiastole: Code<'static> = c("93663-3", "Left ventricular sphericity index end diastole");
pub const LeftVentricularStrokeVolume3D: Code<'static> = c("80035-9", "Left ventricular stroke volume 3D");
pub const LeftVentricularStrokeVolumeAorticRootCalculated: Code<'static> = c("20324-0", "Left ventricular stroke volume (aortic root calculated)");
pub const LeftVentricularStrokeWork: Code<'static> = c("8861-7", "Left Ventricular Stroke Work");
pub const LeftVentricularStrokeWorkIndex: Code<'static> = c("8863-3", "Left Ventricular Stroke Work Index");
pub const LengthOfVertebraTokyo1986: Code<'static> = c("33118-1", "Length of Vertebra, Tokyo 1986");
pub const LiveBirths: Code<'static> = c("11636-8", "Live Births");
pub const MCAPulsatilityIndex: Code<'static> = c("11999-0", "MCA Pulsatility Index");
pub const MR3DCAMModel: Code<'static> = c("85041-2", "MR 3D CAM model");
pub const MRISpineReport: Code<'static> = c("18756-7", "MRI Spine Report");
pub const MRIUnspecifiedBodyRegion: Code<'static> = c("25056-3", "MRI unspecified body region");
pub const MRIWholeBody: Code<'static> = c("46358-8", "MRI whole body");
pub const MacularGridCenterPointThickness: Code<'static> = c("57108-3", "Macular Grid.Center Point Thickness");
pub const MacularGridCenterSubfieldThickness: Code<'static> = c("57109-1", "Macular Grid.Center Subfield Thickness");
pub const MacularGridInnerInferiorSubfieldThickness: Code<'static> = c("57112-5", "Macular Grid.Inner Inferior Subfield Thickness");
pub const MacularGridInnerNasalSubfieldThickness: Code<'static> = c("57111-7", "Macular Grid.Inner Nasal Subfield Thickness");
pub const MacularGridInnerSuperiorSubfieldThickness: Code<'static> = c("57110-9", "Macular Grid.Inner Superior Subfield Thickness");
pub const MacularGridInnerTemporalSubfieldThickness: Code<'static> = c("57113-3", "Macular Grid.Inner Temporal Subfield Thickness");
pub const MacularGridOuterInferiorSubfieldThickness: Code<'static> = c("57116-6", "Macular Grid.Outer Inferior Subfield Thickness");
pub const MacularGridOuterNasalSubfieldThickness: Code<'static> = c("57115-8", "Macular Grid.Outer Nasal Subfield Thickness");
pub const MacularGridOuterSuperiorSubfieldThickness: Code<'static> = c("57114-1", "Macular Grid.Outer Superior Subfield Thickness");
pub const MacularGridOuterTemporalSubfieldThickness: Code<'static> = c("57117-4", "Macular Grid.Outer Temporal Subfield Thickness");
pub const MacularGridTotalVolume: Code<'static> = c("57118-2", "Macular Grid.Total Volume");
pub const MainPulmonaryArteryDiameter_18020_8: Code<'static> = c("18020-8", "Main Pulmonary Artery Diameter");
pub const MainPulmonaryArteryDiameter_80049_0: Code<'static> = c("80049-0", "Main pulmonary artery diameter");
pub const MainPulmonaryArterySWaveVmax: Code<'static> = c("11720-0", "Main pulmonary artery S-wave Vmax");
pub const MaleSingletonBWPByGAArbuckle1993: Code<'static> = c("33186-8", "Male Singleton BWP by GA, Arbuckle 1993");
pub const MaleTwinsBWPByGAArbuckle1993: Code<'static> = c("33199-1", "Male Twins BWP by GA, Arbuckle 1993");
pub const MammographyScreeningReport: Code<'static> = c("24606-6", "Mammography Screening Report");
pub const MeanGradient: Code<'static> = c("20256-4", "Mean Gradient");
pub const MeanVelocityOfCircumferentialFiberShorteningMeanVcFv: Code<'static> = c("59117-2", "Mean Velocity of Circumferential Fiber Shortening (Mean VcFv)");
pub const MedicationsAdministered: Code<'static> = c("29549-3", "Medications Administered");
pub const MinimumDiastolicVelocity: Code<'static> = c("11665-7", "Minimum Diastolic Velocity");
pub const MitralAnnulusDiastolicDiameterA2C: Code<'static> = c("80050-8", "Mitral annulus diastolic diameter - A2C");
pub const MitralAnnulusDiastolicDiameterA4C: Code<'static> = c("80051-6", "Mitral annulus diastolic diameter - A4C");
pub const MitralAnnulusDiastolicDiameterPLAX: Code<'static> = c("80052-4", "Mitral annulus diastolic diameter - PLAX");
pub const MitralAnnulusVTI: Code<'static> = c("80053-2", "Mitral annulus VTI");
pub const MitralLateralAPeakVelocity: Code<'static> = c("81397-2", "Mitral lateral a' peak velocity");
pub const MitralLateralEPeakVelocity: Code<'static> = c("78186-4", "Mitral lateral e' peak velocity");
pub const MitralLateralEPerERatio: Code<'static> = c("78190-6", "Mitral Lateral E/e' ratio");
pub const MitralLateralEPrimeVmax: Code<'static> = c("80054-0", "Mitral lateral e-prime Vmax");
pub const MitralLateralSPeakVelocity: Code<'static> = c("78188-0", "Mitral lateral s' peak velocity");
pub const MitralRegurgitantFlowPISA: Code<'static> = c("80057-3", "Mitral regurgitant flow (PISA)");
pub const MitralRegurgitantFractionContinuityVTI: Code<'static> = c("80055-7", "Mitral regurgitant fraction (Continuity VTI)");
pub const MitralRegurgitantFractionPISA: Code<'static> = c("80056-5", "Mitral regurgitant fraction (PISA)");
pub const MitralRegurgitationDPPerDtDerivedFromMitralRegVelocity: Code<'static> = c("18035-6", "Mitral Regurgitation dP/dt derived from Mitral Reg. velocity");
pub const MitralRegurgitationPISARadius: Code<'static> = c("80059-9", "Mitral regurgitation PISA radius");
pub const MitralRegurgitationPISAVelocity: Code<'static> = c("80060-7", "Mitral regurgitation PISA velocity");
pub const MitralRegurgitationPeakGradient: Code<'static> = c("80058-1", "Mitral regurgitation peak gradient");
pub const MitralRegurgitationVenaContractaWidth: Code<'static> = c("80061-5", "Mitral regurgitation vena contracta width");
pub const MitralRegurgitationVmax: Code<'static> = c("80062-3", "Mitral regurgitation Vmax");
pub const MitralRegurgitationVolumeContinuityVTI: Code<'static> = c("80063-1", "Mitral regurgitation volume (Continuity VTI)");
pub const MitralRegurgitationVolumePISA: Code<'static> = c("80064-9", "Mitral regurgitation volume (PISA)");
pub const MitralSeptalAPeakVelocity: Code<'static> = c("81396-4", "Mitral septal a' peak velocity");
pub const MitralSeptalEPeakVelocity: Code<'static> = c("78185-6", "Mitral septal e' peak velocity");
pub const MitralSeptalEPerERatio: Code<'static> = c("78189-8", "Mitral Septal E/e' ratio");
pub const MitralSeptalEPrimeVmax: Code<'static> = c("79911-4", "Mitral septal e-prime Vmax");
pub const MitralSeptalSPeakVelocity: Code<'static> = c("78187-2", "Mitral septal s' peak velocity");
pub const MitralValveAWaveDuration: Code<'static> = c("80065-6", "Mitral valve A-wave duration");
pub const MitralValveAWavePeakVelocity: Code<'static> = c("17978-8", "Mitral Valve A-Wave Peak Velocity");
pub const MitralValveAWaveVmax: Code<'static> = c("80066-4", "Mitral valve A-wave Vmax");
pub const MitralValveAnnulusArea: Code<'static> = c("20264-8", "Mitral valve annulus area");
pub const MitralValveAnnulusDiameter: Code<'static> = c("18017-4", "Mitral valve annulus diameter");
pub const MitralValveAreaPISA: Code<'static> = c("80067-2", "Mitral valve area (PISA)");
pub const MitralValveAreaPlanimetry: Code<'static> = c("80068-0", "Mitral valve area (Planimetry)");
pub const MitralValveAreaPressureHalfTime: Code<'static> = c("80069-8", "Mitral valve area (Pressure Half-Time)");
pub const MitralValveDecelerationTime: Code<'static> = c("78191-4", "Mitral valve deceleration time");
pub const MitralValveDiastolicPeakInstantaneousGradient: Code<'static> = c("18057-0", "Mitral Valve Diastolic Peak Instantaneous Gradient");
pub const MitralValveEFSlopeByMMode: Code<'static> = c("18040-6", "Mitral Valve E-F Slope by M-Mode");
pub const MitralValveEPSSEWave: Code<'static> = c("18036-4", "Mitral Valve EPSS, E wave");
pub const MitralValveESeptalSeparation: Code<'static> = c("59098-4", "Mitral Valve E-septal Separation");
pub const MitralValveEToARatio: Code<'static> = c("18038-0", "Mitral Valve E to A Ratio");
pub const MitralValveEWavePeakVelocity: Code<'static> = c("18037-2", "Mitral Valve E-Wave Peak Velocity");
pub const MitralValveEWaveVmax: Code<'static> = c("80070-6", "Mitral valve E-wave Vmax");
pub const MitralValveEffectiveRegurgitantOrificeAreaPISA_29448_8: Code<'static> = c("29448-8", "Mitral valve effective regurgitant orifice area (PISA)");
pub const MitralValveEffectiveRegurgitantOrificeAreaPISA_80071_4: Code<'static> = c("80071-4", "Mitral valve effective regurgitant orifice area (PISA)");
pub const MitralValveEffectiveRegurgitantOrificeAreaVolumetric: Code<'static> = c("77914-0", "Mitral valve effective regurgitant orifice area (volumetric)");
pub const MitralValveFlowPropagationVelocityVp: Code<'static> = c("80072-2", "Mitral valve flow propagation velocity (Vp)");
pub const MitralValveMeanGradient: Code<'static> = c("80073-0", "Mitral valve mean gradient");
pub const MitralValvePeakInstantaneousGradient: Code<'static> = c("80074-8", "Mitral valve peak instantaneous gradient");
pub const MitralValvePressureHalfTime: Code<'static> = c("79912-2", "Mitral valve pressure half-time");
pub const MitralValveVTI: Code<'static> = c("79914-8", "Mitral valve VTI");
pub const MitralValveVenaContractaDiameter: Code<'static> = c("77913-2", "Mitral valve vena contracta diameter");
pub const MitralValveVmax: Code<'static> = c("79913-0", "Mitral valve Vmax");
pub const MyocardialPerformanceIndexTei: Code<'static> = c("59099-2", "Myocardial Performance Index (Tei)");
pub const NMUnspecifiedBodyRegion: Code<'static> = c("49118-3", "NM unspecified body region");
pub const NuchalFoldObservation: Code<'static> = c("12102-0", "Nuchal fold observation");
pub const NuchalFoldThickness: Code<'static> = c("12146-7", "Nuchal Fold thickness");
pub const NuchalTranslucency: Code<'static> = c("33069-6", "Nuchal Translucency");
pub const NuclearMedicineThyroidScanReport: Code<'static> = c("30695-1", "Nuclear Medicine Thyroid Scan Report");
pub const NumberOfFetuses: Code<'static> = c("55281-0", "Number of Fetuses");
pub const NumberOfFetusesByUS: Code<'static> = c("11878-6", "Number of Fetuses by US");
pub const NumberOfFolliclesInLeftOvary: Code<'static> = c("11879-4", "Number of follicles in left ovary");
pub const NumberOfFolliclesInRightOvary: Code<'static> = c("11880-2", "Number of follicles in right ovary");
pub const OFDASUM2000: Code<'static> = c("33119-9", "OFD, ASUM 2000");
pub const OFDByGAASUM2000: Code<'static> = c("33178-5", "OFD by GA, ASUM 2000");
pub const OFDByGAChitty1994: Code<'static> = c("33179-3", "OFD by GA, Chitty 1994");
pub const OFDHansmann1985: Code<'static> = c("33544-8", "OFD, Hansmann 1985");
pub const OFDHansmann1986: Code<'static> = c("33120-7", "OFD, Hansmann 1986");
pub const OFDLessoway1998: Code<'static> = c("33121-5", "OFD, Lessoway 1998");
pub const OODMayden1982: Code<'static> = c("33124-9", "OOD, Mayden, 1982");
pub const OODTrout1994: Code<'static> = c("33125-6", "OOD, Trout 1994");
pub const OccipitalFrontalDiameter: Code<'static> = c("11851-3", "Occipital-Frontal Diameter");
pub const OuterOrbitalDiameter: Code<'static> = c("11629-3", "Outer Orbital Diameter");
pub const OvulationDate: Code<'static> = c("11976-8", "Ovulation date");
pub const OxygenPartialPressureAt50PercentSaturationP50: Code<'static> = c("19217-9", "Oxygen partial pressure at 50% saturation (P50)");
pub const PETUnspecifiedBodyRegion: Code<'static> = c("44136-0", "PET unspecified body region");
pub const PETWholeBody: Code<'static> = c("44139-4", "PET whole body");
pub const Para: Code<'static> = c("11977-6", "Para");
pub const PastSurgicalHistory: Code<'static> = c("10167-5", "Past Surgical History");
pub const PatientHeight: Code<'static> = c("8302-2", "Patient Height");
pub const PatientPresentation: Code<'static> = c("55108-5", "Patient Presentation");
pub const PatientWeight: Code<'static> = c("29463-7", "Patient Weight");
pub const PeakBloodVelocity: Code<'static> = c("20355-4", "Peak Blood Velocity");
pub const PeakEWavePerPeakAWaveByUS: Code<'static> = c("59104-0", "Peak E wave/Peak A wave by US");
pub const PeakGradient: Code<'static> = c("20247-3", "Peak Gradient");
pub const PeakInstantaneousFlowRate: Code<'static> = c("34141-2", "Peak Instantaneous Flow Rate");
pub const PeakReversalVelocityDuringAtrialContraction: Code<'static> = c("59079-4", "Peak Reversal Velocity during Atrial Contraction");
pub const PeakSystolicVelocity: Code<'static> = c("11726-7", "Peak Systolic Velocity");
pub const PeakTissueVelocity: Code<'static> = c("59133-9", "Peak Tissue Velocity");
pub const PercentThickening: Code<'static> = c("59092-7", "% Thickening");
pub const PosteriorHornLateralVentricularWidth: Code<'static> = c("33196-7", "Posterior Horn Lateral ventricular width");
pub const PreEjectionPeriod: Code<'static> = c("59085-1", "Pre-Ejection Period");
pub const PreEjectionPeriodPerEjectionTimeRatio: Code<'static> = c("59088-5", "Pre-Ejection Period/Ejection Time Ratio");
pub const PressureHalfTime: Code<'static> = c("20280-4", "Pressure Half-Time");
pub const Preterm: Code<'static> = c("11637-6", "Preterm");
pub const PreviousFindings: Code<'static> = c("18834-2", "Previous Findings");
pub const PriorProcedureDescriptions: Code<'static> = c("55114-3", "Prior Procedure Descriptions");
pub const ProblemList: Code<'static> = c("11450-4", "Problem List");
pub const ProstateCancerAntigen: Code<'static> = c("2857-1", "Prostate Cancer Antigen");
pub const ProstateSpecificAntigenDensity: Code<'static> = c("15325-4", "Prostate specific Antigen Density");
pub const PulmVeinADurationToMVADurationDifference: Code<'static> = c("59114-9", "Pulm Vein A duration to MV A duration difference");
pub const PulmonaryArteryMainOxygenContent: Code<'static> = c("10245-9", "Pulmonary Artery Main Oxygen content");
pub const PulmonaryArteryPressureUsingAccelTime: Code<'static> = c("59101-6", "Pulmonary Artery Pressure using Accel Time");
pub const PulmonaryToSystemicShuntFlowRatio: Code<'static> = c("29462-9", "Pulmonary-to-Systemic Shunt Flow Ratio");
pub const PulmonaryVeinADurationMitralValveADurationRatio: Code<'static> = c("59112-3", "Pulmonary Vein A Duration Mitral Valve A Duration Ratio");
pub const PulmonaryVeinAVTIToMitralValveVTIRatio: Code<'static> = c("59113-1", "Pulmonary Vein A VTI to Mitral Valve VTI Ratio");
pub const PulmonaryVeinAWaveDuration: Code<'static> = c("78184-9", "Pulmonary vein A-wave duration");
pub const PulmonaryVeinAWaveVmax: Code<'static> = c("79915-5", "Pulmonary vein A-wave Vmax");
pub const PulmonaryVeinAtrialContractionReversalPeakVelocity: Code<'static> = c("29453-8", "Pulmonary Vein Atrial Contraction Reversal Peak Velocity");
pub const PulmonaryVeinDWaveVmax: Code<'static> = c("79916-3", "Pulmonary vein D-wave Vmax");
pub const PulmonaryVeinDiastolicPeakVelocity: Code<'static> = c("29451-2", "Pulmonary Vein Diastolic Peak Velocity");
pub const PulmonaryVeinSWaveVmax: Code<'static> = c("79917-1", "Pulmonary vein S-wave Vmax");
pub const PulmonaryVeinSystolicPeakVelocity: Code<'static> = c("29450-4", "Pulmonary Vein Systolic Peak Velocity");
pub const PulmonaryVeinSystolicToDiastolicRatio: Code<'static> = c("29452-0", "Pulmonary Vein Systolic to Diastolic Ratio");
pub const PulmonaryWedgeOxygenContent: Code<'static> = c("10247-5", "Pulmonary Wedge Oxygen content");
pub const PulmonicAnnulusDiameter: Code<'static> = c("79909-8", "Pulmonic annulus diameter");
pub const PulmonicRegurgitationEndDiastolicPeakGradient: Code<'static> = c("79934-6", "Pulmonic regurgitation end diastolic peak gradient");
pub const PulmonicRegurgitationEndDiastolicVelocity: Code<'static> = c("79918-9", "Pulmonic regurgitation end diastolic velocity");
pub const PulmonicRegurgitationVmax: Code<'static> = c("79919-7", "Pulmonic regurgitation Vmax");
pub const PulmonicValveAccelerationTime: Code<'static> = c("79928-8", "Pulmonic valve acceleration time");
pub const PulmonicValveAreaByContinuity: Code<'static> = c("18096-8", "Pulmonic Valve Area by continuity");
pub const PulmonicValveEjectionTime: Code<'static> = c("18042-2", "Pulmonic Valve Ejection Time");
pub const PulmonicValvePeakGradient: Code<'static> = c("79935-3", "Pulmonic valve peak gradient");
pub const PulmonicValveVTI: Code<'static> = c("79910-6", "Pulmonic valve VTI");
pub const PulmonicValveVmax: Code<'static> = c("79920-5", "Pulmonic valve Vmax");
pub const PulsatilityIndex: Code<'static> = c("12008-9", "Pulsatility Index");
pub const ROIInternalDimensionByUS: Code<'static> = c("59090-1", "ROI Internal Dimension by US");
pub const ROIThicknessByUS: Code<'static> = c("59089-3", "ROI Thickness by US");
pub const RVEjectionFraction: Code<'static> = c("10231-9", "RV Ejection Fraction");
pub const RVStrokeVolume: Code<'static> = c("8779-1", "RV Stroke Volume");
pub const RadiationExposureAndProtectionInformation: Code<'static> = c("73569-6", "Radiation Exposure and Protection Information");
pub const RadiologyReport: Code<'static> = c("11528-7", "Radiology Report");
pub const RadionuclideBrainScan: Code<'static> = c("24730-4", "Radionuclide brain scan");
pub const RadionuclideScanOfWholeBody: Code<'static> = c("42175-0", "Radionuclide scan of whole body");
pub const RadiusByGAJeanty1983: Code<'static> = c("33180-1", "Radius by GA, Jeanty 1983");
pub const RadiusJeanty1983: Code<'static> = c("33126-4", "Radius, Jeanty 1983");
pub const RadiusLength: Code<'static> = c("11967-7", "Radius length");
pub const RadiusMerz1987: Code<'static> = c("11939-6", "Radius, Merz 1987");
pub const Recommendations: Code<'static> = c("18783-1", "Recommendations");
pub const RelevantDiagnosticTestsAndPerOrLaboratoryData: Code<'static> = c("30954-2", "Relevant Diagnostic Tests and/or Laboratory Data");
pub const RenalArteryPerAortaVelocityRatio: Code<'static> = c("33869-9", "Renal Artery/Aorta velocity ratio");
pub const Request: Code<'static> = c("55115-0", "Request");
pub const ResistivityIndex: Code<'static> = c("12023-8", "Resistivity Index");
pub const RespirationRhythm: Code<'static> = c("9304-7", "Respiration Rhythm");
pub const RestingHeartRate: Code<'static> = c("40443-4", "Resting Heart Rate");
pub const RightAtrialEndSystolicArea4C: Code<'static> = c("80075-5", "Right atrial end systolic area 4C");
pub const RightAtrialMajorAxisDimension4C: Code<'static> = c("80076-3", "Right atrial major axis dimension 4C");
pub const RightAtrialMinorAxisDimension4C: Code<'static> = c("80077-1", "Right atrial minor axis dimension 4C");
pub const RightAtrialMinorAxisDimension4CPerBSA: Code<'static> = c("80078-9", "Right atrial minor axis dimension 4C / BSA");
pub const RightAtriumAreaA4CView: Code<'static> = c("17988-7", "Right Atrium Area A4C view");
pub const RightAtriumSystolicPressure: Code<'static> = c("18070-3", "Right Atrium Systolic Pressure");
pub const RightFetalEarLength: Code<'static> = c("53668-0", "Right Fetal Ear Length");
pub const RightKidneyLength: Code<'static> = c("11836-4", "Right Kidney length");
pub const RightKidneyThickness: Code<'static> = c("11855-4", "Right Kidney thickness");
pub const RightKidneyWidth: Code<'static> = c("11827-3", "Right Kidney width");
pub const RightOvaryHeight: Code<'static> = c("11858-8", "Right Ovary Height");
pub const RightOvaryLength: Code<'static> = c("11841-4", "Right Ovary Length");
pub const RightOvaryVolume: Code<'static> = c("12165-7", "Right Ovary Volume");
pub const RightOvaryWidth: Code<'static> = c("11830-7", "Right Ovary Width");
pub const RightPulmonaryArteryDiameter_18021_6: Code<'static> = c("18021-6", "Right Pulmonary Artery Diameter");
pub const RightPulmonaryArteryDiameter_80079_7: Code<'static> = c("80079-7", "Right pulmonary artery diameter");
pub const RightVentricularAnteriorWallDiastolicThickness: Code<'static> = c("18153-7", "Right Ventricular Anterior Wall Diastolic Thickness");
pub const RightVentricularAnteriorWallSystolicThickness: Code<'static> = c("18157-8", "Right Ventricular Anterior Wall Systolic Thickness");
pub const RightVentricularBasalDimension4C: Code<'static> = c("80080-5", "Right ventricular basal dimension 4C");
pub const RightVentricularEDVolume: Code<'static> = c("8822-9", "Right Ventricular ED Volume");
pub const RightVentricularESVolume: Code<'static> = c("8824-5", "Right Ventricular ES Volume");
pub const RightVentricularEjectionFractionByAngiography: Code<'static> = c("8815-3", "Right Ventricular Ejection Fraction by Angiography");
pub const RightVentricularEjectionFractionByCT: Code<'static> = c("8817-9", "Right ventricular ejection fraction by CT");
pub const RightVentricularEjectionFractionByMR: Code<'static> = c("8818-7", "Right ventricular ejection fraction by MR");
pub const RightVentricularEjectionTime: Code<'static> = c("79929-6", "Right ventricular ejection time");
pub const RightVentricularEndDiastolicArea4C: Code<'static> = c("80081-3", "Right ventricular end diastolic area 4C");
pub const RightVentricularEndSystolicArea4C: Code<'static> = c("80082-1", "Right ventricular end systolic area 4C");
pub const RightVentricularFractionalAreaChange: Code<'static> = c("79936-1", "Right ventricular fractional area change");
pub const RightVentricularFreeWallThickness2D: Code<'static> = c("80083-9", "Right ventricular free wall thickness 2D");
pub const RightVentricularFreeWallThicknessMM: Code<'static> = c("80084-7", "Right ventricular free wall thickness MM");
pub const RightVentricularInternalDiastolicDimension: Code<'static> = c("20304-2", "Right Ventricular Internal Diastolic Dimension");
pub const RightVentricularInternalSystolicDimension: Code<'static> = c("20305-9", "Right Ventricular Internal Systolic Dimension");
pub const RightVentricularIntrachamberSystolicPressure: Code<'static> = c("82341-9", "Right ventricular Intrachamber systolic pressure");
pub const RightVentricularMidCavityDimension4C: Code<'static> = c("80085-4", "Right ventricular mid-cavity dimension 4C");
pub const RightVentricularMyocardialPerformanceIndex: Code<'static> = c("80086-2", "Right ventricular myocardial performance index");
pub const RightVentricularOutflowTractDiameterAtPulmonicValveRVOTDistal: Code<'static> = c("80087-0", "Right ventricular outflow tract diameter at pulmonic valve (RVOT-Distal)");
pub const RightVentricularOutflowTractDiameterAtSubvalvularLevelRVOTProximal: Code<'static> = c("80088-8", "Right ventricular outflow tract diameter at subvalvular level (RVOT-Proximal)");
pub const RightVentricularOutflowTractVTI: Code<'static> = c("80089-6", "Right ventricular outflow tract VTI");
pub const RightVentricularPreEjectionPeriod: Code<'static> = c("80090-4", "Right ventricular pre-ejection period");
pub const RightVentricularStrokeWork: Code<'static> = c("8862-5", "Right Ventricular Stroke Work");
pub const RightVentricularStrokeWorkIndex: Code<'static> = c("8864-1", "Right Ventricular Stroke Work Index");
pub const SPECTBrain: Code<'static> = c("39632-5", "SPECT brain");
pub const SecondQuadrantDiameter: Code<'static> = c("11626-9", "Second Quadrant Diameter");
pub const SerumCreatinine: Code<'static> = c("2160-0", "Serum Creatinine");
pub const ShePerHerPerHerPerHersPerHerself: Code<'static> = c("LA29519-8", "she/her/her/hers/herself");
pub const SocialHistory: Code<'static> = c("29762-2", "Social History");
pub const SpineLength: Code<'static> = c("33071-2", "Spine Length");
pub const SpineLengthTokyo1989: Code<'static> = c("33127-2", "Spine Length, Tokyo, 1989");
pub const Stage: Code<'static> = c("18139-6", "Stage");
pub const Status: Code<'static> = c("33999-4", "Status");
pub const StenosisPeakGradient: Code<'static> = c("59106-5", "Stenosis Peak Gradient");
pub const StenosisPeakVelocity: Code<'static> = c("59107-3", "Stenosis Peak Velocity");
pub const Summary: Code<'static> = c("55112-7", "Summary");
pub const SystolicToDiastolicVelocityRatio: Code<'static> = c("12144-2", "Systolic to Diastolic Velocity Ratio");
pub const TADEriksen1985: Code<'static> = c("33128-0", "TAD, Eriksen 1985");
pub const TADHansmann1979: Code<'static> = c("33129-8", "TAD Hansmann, 1979");
pub const TADTokyo1986: Code<'static> = c("33130-6", "TAD, Tokyo 1986");
pub const TCDByGAGoldstein1987: Code<'static> = c("33181-9", "TCD by GA Goldstein 1987");
pub const TCDChitty1994: Code<'static> = c("33132-2", "TCD, Chitty 1994");
pub const TCDGoldstein1987: Code<'static> = c("33133-0", "TCD, Goldstein 1987");
pub const TCDHill1990: Code<'static> = c("33134-8", "TCD, Hill 1990");
pub const TCDNimrod1986: Code<'static> = c("33135-5", "TCD, Nimrod 1986");
pub const Term: Code<'static> = c("11639-2", "Term");
pub const ThCChitkara1987: Code<'static> = c("33131-4", "ThC, Chitkara 1987");
pub const TheyPerThemPerTheirPerTheirsPerThemselves: Code<'static> = c("LA29520-6", "they/them/their/theirs/themselves");
pub const ThirdQuadrantDiameter: Code<'static> = c("11625-1", "Third Quadrant Diameter");
pub const ThoracicAortaCoarctationSystolicPeakInstantaneousGradient: Code<'static> = c("17995-2", "Thoracic Aorta Coarctation Systolic Peak Instantaneous Gradient");
pub const ThoracicAortaCoarctationSystolicPeakVelocity: Code<'static> = c("29460-3", "Thoracic Aorta Coarctation Systolic Peak Velocity");
pub const ThoracicArea: Code<'static> = c("33068-8", "Thoracic Area");
pub const ThoracicCircumference: Code<'static> = c("11988-3", "Thoracic Circumference");
pub const TibiaJeanty1984: Code<'static> = c("11941-2", "Tibia, Jeanty 1984");
pub const TibiaLength: Code<'static> = c("11968-5", "Tibia length");
pub const TibialPerBrachialIndex: Code<'static> = c("8581-1", "Tibial/brachial index");
pub const TimeAveragedMeanVelocity: Code<'static> = c("20352-1", "Time averaged mean velocity");
pub const TimeAveragedPeakVelocity: Code<'static> = c("11692-1", "Time averaged peak velocity");
pub const TimeFromQWaveToPulmonicValveCloses: Code<'static> = c("20295-2", "Time from Q wave to Pulmonic Valve Closes");
pub const TimeFromQWaveToTricuspidValveOpens: Code<'static> = c("20296-0", "Time from Q wave to Tricuspid Valve Opens");
pub const TimeToLeftVentricleETissueVelocity: Code<'static> = c("59096-8", "Time to Left Ventricle E Tissue Velocity");
pub const TimeToLeftVentricleSTissueVelocity: Code<'static> = c("59095-0", "Time to Left Ventricle S Tissue Velocity");
pub const TimeToPeakByUS: Code<'static> = c("59121-4", "Time to Peak by US");
pub const TissueVelocityTimeIntegralVTIForTheAreaUnderLeftVentricleAWave: Code<'static> = c("59125-5", "Tissue Velocity Time Integral (VTI) for the area under Left Ventricle A wave");
pub const TissueVelocityTimeIntegralVTIForTheAreaUnderLeftVentricleEWave: Code<'static> = c("59124-8", "Tissue Velocity Time Integral (VTI) for the area under Left Ventricle E wave");
pub const TransverseAbdominalDiameter: Code<'static> = c("11862-0", "Transverse Abdominal Diameter");
pub const TransverseCerebellarDiameter: Code<'static> = c("11863-8", "Transverse Cerebellar Diameter");
pub const TransverseThoracicDiameter: Code<'static> = c("11864-6", "Transverse Thoracic Diameter");
pub const TransverseThoracicDiameterHansmann1985: Code<'static> = c("33136-3", "Transverse Thoracic Diameter, Hansmann 1985");
pub const TransverseThoracicDiameterLessoway1998: Code<'static> = c("33137-1", "Transverse Thoracic Diameter, Lessoway 1998");
pub const TricuspidAnnularPlaneSystolicExcursionTAPSE: Code<'static> = c("77903-3", "Tricuspid Annular Plane Systolic Excursion (TAPSE)");
pub const TricuspidAnnulusDiameter: Code<'static> = c("80091-2", "Tricuspid annulus diameter");
pub const TricuspidRegurgitationDPPerDt: Code<'static> = c("18034-9", "Tricuspid Regurgitation dP/dt");
pub const TricuspidRegurgitationPISARadius: Code<'static> = c("79932-0", "Tricuspid regurgitation PISA radius");
pub const TricuspidRegurgitationPeakGradient: Code<'static> = c("79937-9", "Tricuspid regurgitation peak gradient");
pub const TricuspidRegurgitationVenaContractaWidth: Code<'static> = c("79933-8", "Tricuspid regurgitation vena contracta width");
pub const TricuspidRegurgitationVmax: Code<'static> = c("79921-3", "Tricuspid regurgitation Vmax");
pub const TricuspidValveAPrimeVmax: Code<'static> = c("79922-1", "Tricuspid valve a-prime Vmax");
pub const TricuspidValveAWavePeakVelocity: Code<'static> = c("18030-7", "Tricuspid Valve A Wave Peak Velocity");
pub const TricuspidValveAWaveVmax: Code<'static> = c("79923-9", "Tricuspid valve A-wave Vmax");
pub const TricuspidValveAnnulusDiameter: Code<'static> = c("18023-2", "Tricuspid valve annulus diameter");
pub const TricuspidValveAnnulusRegionCrossectionArea: Code<'static> = c("20344-8", "Tricuspid valve annulus region crossection area");
pub const TricuspidValveClosureToOpeningTime: Code<'static> = c("79930-4", "Tricuspid valve closure to opening time");
pub const TricuspidValveDecelerationTime: Code<'static> = c("79931-2", "Tricuspid valve deceleration time");
pub const TricuspidValveDiastolicVTI: Code<'static> = c("18175-0", "Tricuspid valve diastolic VTI");
pub const TricuspidValveEPrimeVmax: Code<'static> = c("79924-7", "Tricuspid valve e-prime Vmax");
pub const TricuspidValveEToARatio: Code<'static> = c("18039-8", "Tricuspid Valve E to A Ratio");
pub const TricuspidValveEWavePeakVelocity: Code<'static> = c("18031-5", "Tricuspid Valve E Wave Peak Velocity");
pub const TricuspidValveEWaveVmax: Code<'static> = c("79925-4", "Tricuspid valve E-wave Vmax");
pub const TricuspidValveEffectiveRegurgitantOrificeAreaPISA: Code<'static> = c("81093-7", "Tricuspid valve effective regurgitant orifice area (PISA)");
pub const TricuspidValveEffectiveRegurgitantOrificeAreaVolumetric: Code<'static> = c("81094-5", "Tricuspid valve effective regurgitant orifice area (Volumetric)");
pub const TricuspidValveMeanGradient: Code<'static> = c("79938-7", "Tricuspid valve mean gradient");
pub const TricuspidValvePeakGradient: Code<'static> = c("79939-5", "Tricuspid valve peak gradient");
pub const TricuspidValvePressureHalfTime: Code<'static> = c("18032-3", "Tricuspid valve pressure half-time");
pub const TricuspidValveSPrimeVmax: Code<'static> = c("79926-2", "Tricuspid valve s-prime Vmax");
pub const TricuspidValveVmax: Code<'static> = c("79927-0", "Tricuspid valve Vmax");
pub const UAPulsatilityIndex: Code<'static> = c("12003-0", "UA Pulsatility Index");
pub const UAResistivityIndex: Code<'static> = c("12018-8", "UA Resistivity Index");
pub const USArteryReport: Code<'static> = c("46375-2", "US Artery Report");
pub const USBreastReport: Code<'static> = c("24601-7", "US Breast Report");
pub const USCarotidArteriesReport: Code<'static> = c("24616-5", "US Carotid Arteries Report");
pub const USDopplerAbdominalVesselsReport: Code<'static> = c("24534-0", "US Doppler Abdominal Vessels Report");
pub const USDopplerExtremityVesselsReport: Code<'static> = c("43771-5", "US Doppler Extremity Vessels Report");
pub const USDopplerHeadVesselsReport: Code<'static> = c("24733-8", "US Doppler Head Vessels Report");
pub const USDopplerLowerExtremityVesselsReport: Code<'static> = c("44174-1", "US Doppler Lower Extremity Vessels Report");
pub const USDopplerUpperExtremityVesselsReport: Code<'static> = c("39448-6", "US Doppler Upper Extremity Vessels Report");
pub const USDopplerVesselsReport: Code<'static> = c("39445-2", "US Doppler Vessels Report");
pub const USKidneyReport: Code<'static> = c("38036-0", "US Kidney Report");
pub const USLiverReport: Code<'static> = c("28614-6", "US Liver Report");
pub const USPancreasReport: Code<'static> = c("24859-1", "US Pancreas Report");
pub const USPelvis: Code<'static> = c("24869-0", "US Pelvis");
pub const USProstateTransrectalReport: Code<'static> = c("24884-9", "US Prostate (transrectal) Report");
pub const USScrotumAndTesticleReport: Code<'static> = c("25002-7", "US Scrotum and Testicle Report");
pub const USShoulderReport: Code<'static> = c("24907-8", "US Shoulder Report");
pub const USSpleenReport: Code<'static> = c("24990-4", "US Spleen Report");
pub const USTendonReport: Code<'static> = c("39453-6", "US Tendon Report");
pub const USThyroidReport: Code<'static> = c("25010-0", "US Thyroid Report");
pub const USVeinReport: Code<'static> = c("39036-9", "US Vein Report");
pub const UlnaJeanty1984: Code<'static> = c("11944-6", "Ulna, Jeanty 1984");
pub const UlnaLength: Code<'static> = c("11969-3", "Ulna length");
pub const UlnaMerz1987: Code<'static> = c("11945-3", "Ulna, Merz 1987");
pub const UltrasoundObstetricAndGynReport: Code<'static> = c("11525-3", "Ultrasound Obstetric and Gyn Report");
pub const UltrasoundReport: Code<'static> = c("25061-3", "Ultrasound Report");
pub const UniqueDeviceIdentifier: Code<'static> = c("74711-3", "Unique Device Identifier");
pub const UterusHeight: Code<'static> = c("11859-6", "Uterus Height");
pub const UterusLength: Code<'static> = c("11842-2", "Uterus Length");
pub const UterusVolume: Code<'static> = c("33192-6", "Uterus Volume");
pub const UterusWidth: Code<'static> = c("11865-3", "Uterus Width");
pub const VelocityOfFlowPropagation: Code<'static> = c("59115-6", "Velocity of Flow Propagation");
pub const VelocityRatio: Code<'static> = c("33867-3", "Velocity ratio");
pub const VelocityTimeIntegral: Code<'static> = c("20354-7", "Velocity Time Integral");
pub const VenousBloodBaseExcess: Code<'static> = c("1927-3", "Venous Blood Base Excess");
pub const VenousBloodCarbonDioxideContent: Code<'static> = c("2027-1", "Venous Blood Carbon dioxide content");
pub const VenousBloodCarbonDioxidePartialPressure: Code<'static> = c("2021-4", "Venous Blood Carbon dioxide partial pressure");
pub const VenousBloodPH: Code<'static> = c("2746-6", "Venous Blood pH");
pub const VenousOxygenContent: Code<'static> = c("19220-3", "Venous Oxygen content");
pub const VenousOxygenPartialPressure: Code<'static> = c("2705-2", "Venous Oxygen partial pressure");
pub const VenousOxygenPartialPressureAt50PercentSaturation: Code<'static> = c("19216-1", "Venous Oxygen partial pressure at 50% saturation");
pub const VenousOxygenSaturation: Code<'static> = c("2711-0", "Venous Oxygen saturation");
pub const VitalSigns: Code<'static> = c("8716-3", "Vital Signs");
pub const VolumeFlow: Code<'static> = c("33878-0", "Volume flow");
pub const WallSegment: Code<'static> = c("18179-2", "Wall Segment");
pub const WholeBodyCT: Code<'static> = c("46305-9", "Whole body CT");
pub const WidthOfHemisphere: Code<'static> = c("12170-7", "Width of Hemisphere");
pub const XRUnspecifiedBodyRegion: Code<'static> = c("43468-8", "XR unspecified body region");
pub const YolkSacLength: Code<'static> = c("11816-6", "Yolk Sac length");
pub fn by_name(name: &str) -> Option<Code<'static>> {
    super::find_named(&NAMED, name)
}
#[allow(deprecated)]
static NAMED: [(&str, Code<'static>); 893] = [
    ("ACASUM2000", ACASUM2000),
    ("ACByGAASUM2000", ACByGAASUM2000),
    ("ACByGAHadlock1984", ACByGAHadlock1984),
    ("ACByGAMerz1988", ACByGAMerz1988),
    ("ACByGAShinozuka1996", ACByGAShinozuka1996),
    ("ACCampbell1975", ACCampbell1975),
    ("ACDerivedByGAChitty1994", ACDerivedByGAChitty1994),
    ("ACHadlock1984", ACHadlock1984),
    ("ACHansmann1985", ACHansmann1985),
    ("ACInterval", ACInterval),
    ("ACJeanty1982", ACJeanty1982),
    ("ACJeanty1984", ACJeanty1984),
    ("ACLessoway1998", ACLessoway1998),
    ("ACMeasuredByGAChitty1994", ACMeasuredByGAChitty1994),
    ("ACMertz1988", ACMertz1988),
    ("ACShinozuka1996", ACShinozuka1996),
    ("APADTAD", APADTAD),
    ("APAbdominalDiameterLessoway1998", APAbdominalDiameterLessoway1998),
    ("AWaveAmplitude", AWaveAmplitude),
    ("AWaveDuration", AWaveDuration),
    ("AWavePeakVelocity", AWavePeakVelocity),
    ("AbdominalCircumference", AbdominalCircumference),
    ("AbdominalWallObservation", AbdominalWallObservation),
    ("Abortus", Abortus),
    ("AccelerationIndex", AccelerationIndex),
    ("AccelerationTime", AccelerationTime),
    ("Addendum", Addendum),
    ("AliasVelocity", AliasVelocity),
    ("AmnioticFluidIndex", AmnioticFluidIndex),
    ("AmnioticFluidVolume", AmnioticFluidVolume),
    ("AnteriorHornLateralVentricularWidth", AnteriorHornLateralVentricularWidth),
    ("AnteriorMitralValveLeafletLength", AnteriorMitralValveLeafletLength),
    ("AnteriorPosteriorAbdominalDiameter", AnteriorPosteriorAbdominalDiameter),
    ("AnteriorPosteriorTrunkDiameter", AnteriorPosteriorTrunkDiameter),
    ("AortaSinotubularJunctionDiameterBy2D", AortaSinotubularJunctionDiameterBy2D),
    ("AortaSinotubularJunctionDiameterByMMode", AortaSinotubularJunctionDiameterByMMode),
    ("AorticAnnulusDiameter", AorticAnnulusDiameter),
    ("AorticArchDiameter", AorticArchDiameter),
    ("AorticIsthmusDiameter", AorticIsthmusDiameter),
    ("AorticRegurgitantFlow", AorticRegurgitantFlow),
    ("AorticRegurgitantFraction", AorticRegurgitantFraction),
    ("AorticRegurgitantJetAreaPerLVOTAreaPercent", AorticRegurgitantJetAreaPerLVOTAreaPercent),
    ("AorticRegurgitantJetWidthPerLVOTWidthPercent", AorticRegurgitantJetWidthPerLVOTWidthPercent),
    ("AorticRegurgitationPISARadius", AorticRegurgitationPISARadius),
    ("AorticRegurgitationPISAVelocity", AorticRegurgitationPISAVelocity),
    ("AorticRegurgitationPressureHalfTime", AorticRegurgitationPressureHalfTime),
    ("AorticRegurgitationVTI", AorticRegurgitationVTI),
    ("AorticRegurgitationVenaContractaWidth", AorticRegurgitationVenaContractaWidth),
    ("AorticRegurgitationVmax", AorticRegurgitationVmax),
    ("AorticRegurgitationVolumeContinuityVTI", AorticRegurgitationVolumeContinuityVTI),
    ("AorticRegurgitationVolumePISA", AorticRegurgitationVolumePISA),
    ("AorticRootDiameterPerBSA", AorticRootDiameterPerBSA),
    ("AorticRootDiameter_18015_8", AorticRootDiameter_18015_8),
    ("AorticRootDiameter_79953_6", AorticRootDiameter_79953_6),
    ("AorticRootOxygenContent", AorticRootOxygenContent),
    ("AorticSinotubularJunctionDimension", AorticSinotubularJunctionDimension),
    ("AorticSinotubularJunctionToAorticRootRatio", AorticSinotubularJunctionToAorticRootRatio),
    ("AorticValveAnnulusDiameter", AorticValveAnnulusDiameter),
    ("AorticValveAreaContinuityVTI", AorticValveAreaContinuityVTI),
    ("AorticValveAreaContinuityVTIPerBSA", AorticValveAreaContinuityVTIPerBSA),
    ("AorticValveAreaContinuityVmax", AorticValveAreaContinuityVmax),
    ("AorticValveAreaContinuityVmaxPerBSA", AorticValveAreaContinuityVmaxPerBSA),
    ("AorticValveCuspSeparation", AorticValveCuspSeparation),
    ("AorticValveEffectiveRegurgitantOrificeArea", AorticValveEffectiveRegurgitantOrificeArea),
    ("AorticValveEffectiveRegurgitantOrificeAreaPISA", AorticValveEffectiveRegurgitantOrificeAreaPISA),
    ("AorticValveEffectiveRegurgitantOrificeAreaVolumetric", AorticValveEffectiveRegurgitantOrificeAreaVolumetric),
    ("AorticValveEjectionTime", AorticValveEjectionTime),
    ("AorticValveMeanBloodVelocity", AorticValveMeanBloodVelocity),
    ("AorticValveMeanGradient", AorticValveMeanGradient),
    ("AorticValveOrificeArea", AorticValveOrificeArea),
    ("AorticValveOrificeAreaContinuity", AorticValveOrificeAreaContinuity),
    ("AorticValveOrificeAreaContinuityVTIPlusArea", AorticValveOrificeAreaContinuityVTIPlusArea),
    ("AorticValveOrificeAreaContinuityVTIPlusDiameter", AorticValveOrificeAreaContinuityVTIPlusDiameter),
    ("AorticValveOrificeAreaContinuityVmaxPlusArea", AorticValveOrificeAreaContinuityVmaxPlusArea),
    ("AorticValveOrificeAreaContinuityVmaxPlusDiameter", AorticValveOrificeAreaContinuityVmaxPlusDiameter),
    ("AorticValvePeakInstantaneousGradient", AorticValvePeakInstantaneousGradient),
    ("AorticValvePressureHalfTime", AorticValvePressureHalfTime),
    ("AorticValveRegurgitantBloodFlowPressureHalfTime", AorticValveRegurgitantBloodFlowPressureHalfTime),
    ("AorticValveVTI", AorticValveVTI),
    ("AorticValveVenaContractaDiameter", AorticValveVenaContractaDiameter),
    ("AorticValveVmax", AorticValveVmax),
    ("ArterialBloodBaseExcess", ArterialBloodBaseExcess),
    ("ArterialBloodCarbonDioxideContent", ArterialBloodCarbonDioxideContent),
    ("ArterialBloodCarbonDioxidePartialPressure", ArterialBloodCarbonDioxidePartialPressure),
    ("ArterialBloodPH", ArterialBloodPH),
    ("ArterialOxygenContent", ArterialOxygenContent),
    ("ArterialOxygenPartialPressure", ArterialOxygenPartialPressure),
    ("ArterialOxygenPartialPressureAt50PercentSaturation", ArterialOxygenPartialPressureAt50PercentSaturation),
    ("ArterialOxygenSaturation", ArterialOxygenSaturation),
    ("AscendingAortaDimension", AscendingAortaDimension),
    ("AscendingAorticDiameter", AscendingAorticDiameter),
    ("AverageUltrasoundAge", AverageUltrasoundAge),
    ("AxTByGAShinozuka1996", AxTByGAShinozuka1996),
    ("AxTShinozuka1996", AxTShinozuka1996),
    ("BCSlope", BCSlope),
    ("BDJeanty1982", BDJeanty1982),
    ("BPDASUM1989", BPDASUM1989),
    ("BPDAreaCorrected", BPDAreaCorrected),
    ("BPDByGAASUM2000", BPDByGAASUM2000),
    ("BPDByGAHadlock1984", BPDByGAHadlock1984),
    ("BPDByGAJeanty1982", BPDByGAJeanty1982),
    ("BPDByGAMerz1988", BPDByGAMerz1988),
    ("BPDByGARempen1991", BPDByGARempen1991),
    ("BPDByGAShinozuka1996", BPDByGAShinozuka1996),
    ("BPDDoubilet1993", BPDDoubilet1993),
    ("BPDHadlock1984", BPDHadlock1984),
    ("BPDHansmann1985", BPDHansmann1985),
    ("BPDHansmann1986", BPDHansmann1986),
    ("BPDJeanty1982", BPDJeanty1982),
    ("BPDJeanty1984", BPDJeanty1984),
    ("BPDKurtz1980", BPDKurtz1980),
    ("BPDLessoway1998", BPDLessoway1998),
    ("BPDMertz1988", BPDMertz1988),
    ("BPDOiChitty1997", BPDOiChitty1997),
    ("BPDOoChitty1997", BPDOoChitty1997),
    ("BPDOsaka1989", BPDOsaka1989),
    ("BPDOuterInnerByGAChitty1994", BPDOuterInnerByGAChitty1994),
    ("BPDOuterOuterByGAChitty1994", BPDOuterOuterByGAChitty1994),
    ("BPDRempen1991", BPDRempen1991),
    ("BPDSabbagha1978", BPDSabbagha1978),
    ("BPDShinozuka1996", BPDShinozuka1996),
    ("BPDTokyo1986", BPDTokyo1986),
    ("BPDaHadlock1982", BPDaHadlock1982),
    ("BSA", BSA),
    ("BiophysicalProfileSumScore", BiophysicalProfileSumScore),
    ("BiparietalDiameter", BiparietalDiameter),
    ("BloodBaseExcess", BloodBaseExcess),
    ("BloodCarbonDioxideContent", BloodCarbonDioxideContent),
    ("BloodCarbonDioxidePartialPressure", BloodCarbonDioxidePartialPressure),
    ("BloodOxygenPartialPressure", BloodOxygenPartialPressure),
    ("BloodOxygenSaturation", BloodOxygenSaturation),
    ("BloodPH", BloodPH),
    ("BodySurfaceAreaFormula", BodySurfaceAreaFormula),
    ("BodyTemperature", BodyTemperature),
    ("BrainMRI", BrainMRI),
    ("BrainPET", BrainPET),
    ("BreastBilateralMammogram", BreastBilateralMammogram),
    ("BreastBilateralMr", BreastBilateralMr),
    ("BreastBilateralMrWContrastIv", BreastBilateralMrWContrastIv),
    ("BreastBilateralMrWoAndWContrastIv", BreastBilateralMrWoAndWContrastIv),
    ("BreastFfdMammogram", BreastFfdMammogram),
    ("BreastLeftMammogram", BreastLeftMammogram),
    ("BreastLeftMr", BreastLeftMr),
    ("BreastLeftMrWContrastIv", BreastLeftMrWContrastIv),
    ("BreastLeftMrWoAndWContrastIv", BreastLeftMrWoAndWContrastIv),
    ("BreastMrWContrastIv", BreastMrWContrastIv),
    ("BreastMrWoAndWContrastIv", BreastMrWoAndWContrastIv),
    ("BreastRightMammogram", BreastRightMammogram),
    ("BreastRightMr", BreastRightMr),
    ("BreastRightMrWContrastIv", BreastRightMrWContrastIv),
    ("BreastRightMrWoAndWContrastIv", BreastRightMrWoAndWContrastIv),
    ("BreastUnilateralMammogram", BreastUnilateralMammogram),
    ("BreastUnilateralMr", BreastUnilateralMr),
    ("BreastUnilateralMrWContrastIv", BreastUnilateralMrWContrastIv),
    ("BreastUnilateralMrWoAndWContrastIv", BreastUnilateralMrWoAndWContrastIv),
    ("BreastUnilateralMrWoContrast", BreastUnilateralMrWoContrast),
    ("CEDistance", CEDistance),
    ("CKDEPIFormulaEstimationOfGFR", CKDEPIFormulaEstimationOfGFR),
    ("CRLASUM1991", CRLASUM1991),
    ("CRLASUM2000", CRLASUM2000),
    ("CRLByGAASUM2000", CRLByGAASUM2000),
    ("CRLByGARempen1991", CRLByGARempen1991),
    ("CRLByGAShinozuka1996", CRLByGAShinozuka1996),
    ("CRLDaya1993", CRLDaya1993),
    ("CRLHadlock1992", CRLHadlock1992),
    ("CRLHansmann1985", CRLHansmann1985),
    ("CRLHansmann1986", CRLHansmann1986),
    ("CRLJeanty1982", CRLJeanty1982),
    ("CRLJeanty1984", CRLJeanty1984),
    ("CRLNelson1981", CRLNelson1981),
    ("CRLOsaka1989", CRLOsaka1989),
    ("CRLRempen1991", CRLRempen1991),
    ("CRLRobinson1975", CRLRobinson1975),
    ("CRLShinozuka1996", CRLShinozuka1996),
    ("CRLTokyo1986", CRLTokyo1986),
    ("CT3DCAMModel", CT3DCAMModel),
    ("CTAbdomenReport", CTAbdomenReport),
    ("CTChestReport", CTChestReport),
    ("CTPerfusionHeadWithContrastIV", CTPerfusionHeadWithContrastIV),
    ("CTUnspecifiedBodyRegion", CTUnspecifiedBodyRegion),
    ("CapillaryBloodOxygenSaturation", CapillaryBloodOxygenSaturation),
    ("CapillaryBloodOxygenSaturationByOximetry", CapillaryBloodOxygenSaturationByOximetry),
    ("CardiacCatheterizationReport", CardiacCatheterizationReport),
    ("CardiacCircumferenceTransverseByUS", CardiacCircumferenceTransverseByUS),
    ("CardiacCrossSectionalAreaTransverseByUS", CardiacCrossSectionalAreaTransverseByUS),
    ("CardiacElectrophysiologyReport", CardiacElectrophysiologyReport),
    ("CardiacOutputByIndicatorDilution", CardiacOutputByIndicatorDilution),
    ("CardiacRhythm", CardiacRhythm),
    ("CardiothoracicAreaRatio", CardiothoracicAreaRatio),
    ("CardiothoracicCircumferenceRatio", CardiothoracicCircumferenceRatio),
    ("CephalicIndex", CephalicIndex),
    ("CephalicIndexByGAChitty1994", CephalicIndexByGAChitty1994),
    ("CephalicIndexByGAHadlock1981", CephalicIndexByGAHadlock1981),
    ("CervixLength", CervixLength),
    ("CholesterolInHDL", CholesterolInHDL),
    ("CholesterolInLDL", CholesterolInLDL),
    ("CisternaMagnaLength", CisternaMagnaLength),
    ("ClavicleLength", ClavicleLength),
    ("ClavicleLengthYarkoni1985", ClavicleLengthYarkoni1985),
    ("ClinicalInformation", ClinicalInformation),
    ("ClosureToOpeningTime", ClosureToOpeningTime),
    ("CockroftGaultFormulaEstimationOfGFR", CockroftGaultFormulaEstimationOfGFR),
    ("CommunicationOfCriticalResults", CommunicationOfCriticalResults),
    ("Complications", Complications),
    ("CompositeUltrasoundAge", CompositeUltrasoundAge),
    ("ConceptionDate", ConceptionDate),
    ("Conclusions", Conclusions),
    ("CrownRumpLength", CrownRumpLength),
    ("CurrentProcedureDescriptions", CurrentProcedureDescriptions),
    ("DEExcursion", DEExcursion),
    ("DESlope", DESlope),
    ("DEXASkeletalSystemStudyReport", DEXASkeletalSystemStudyReport),
    ("DPPerDtByUS", DPPerDtByUS),
    ("DecelerationSlope", DecelerationSlope),
    ("DecelerationTime", DecelerationTime),
    ("DeliveryDateEstimatedFromConceptionDate", DeliveryDateEstimatedFromConceptionDate),
    ("DeliveryDateEstimatedFromDateFundalHeightReachUmbilicus", DeliveryDateEstimatedFromDateFundalHeightReachUmbilicus),
    ("DeliveryDateEstimatedFromPhysicalExamination", DeliveryDateEstimatedFromPhysicalExamination),
    ("DeliveryDateEstimatedFromPriorGestationalAgeAssessment", DeliveryDateEstimatedFromPriorGestationalAgeAssessment),
    ("DeliveryDateEstimatedFromQuickeningDate", DeliveryDateEstimatedFromQuickeningDate),
    ("DescendingAorticDiameter", DescendingAorticDiameter),
    ("DiagnosticImagingReport", DiagnosticImagingReport),
    ("DiagnosticInterventionalRadiologyReport", DiagnosticInterventionalRadiologyReport),
    ("ECGOverallFinding", ECGOverallFinding),
    ("ECGReport_11524_0", ECGReport_11524_0),
    ("ECGReport_28010_7", ECGReport_28010_7),
    ("EDD", EDD),
    ("EDDFromAverageUltrasoundAge", EDDFromAverageUltrasoundAge),
    ("EDDFromLMP", EDDFromLMP),
    ("EDDFromOvulationDate", EDDFromOvulationDate),
    ("EFSlope", EFSlope),
    ("EFW1ByShinozuka1996", EFW1ByShinozuka1996),
    ("EFW2ByShinozuka1996", EFW2ByShinozuka1996),
    ("EFW3ByShinozuka1996", EFW3ByShinozuka1996),
    ("EFWByACAndBPDShepard1982", EFWByACAndBPDShepard1982),
    ("EFWByACBPDFLHCHadlock1985", EFWByACBPDFLHCHadlock1985),
    ("EFWByACBPDFLHadlock1984", EFWByACBPDFLHadlock1984),
    ("EFWByACBPDFLHadlock1985", EFWByACBPDFLHadlock1985),
    ("EFWByACBPDHadlock1984", EFWByACBPDHadlock1984),
    ("EFWByACCampbell1975", EFWByACCampbell1975),
    ("EFWByACFLHCHadlock1985", EFWByACFLHCHadlock1985),
    ("EFWByACFLHadlock1984", EFWByACFLHadlock1984),
    ("EFWByACFLHadlock1985", EFWByACFLHadlock1985),
    ("EFWByACHCHadlock1984", EFWByACHCHadlock1984),
    ("EFWByBPDAPADTADFLTokyo1987", EFWByBPDAPADTADFLTokyo1987),
    ("EFWByBPDFTAFLOsaka1990", EFWByBPDFTAFLOsaka1990),
    ("EFWByBPDTTDHansmann1986", EFWByBPDTTDHansmann1986),
    ("EFWByGAHadlock1991", EFWByGAHadlock1991),
    ("EFWByGAHansmann1986", EFWByGAHansmann1986),
    ("EFWDiscordance", EFWDiscordance),
    ("EFWPercentileRank", EFWPercentileRank),
    ("EFWRatio", EFWRatio),
    ("EVelocityToAnnulusEVelocityRatio", EVelocityToAnnulusEVelocityRatio),
    ("EWavePeakVelocity", EWavePeakVelocity),
    ("EchocardiographyReport", EchocardiographyReport),
    ("EctopicPregnancies", EctopicPregnancies),
    ("EjectionTime", EjectionTime),
    ("EndDiastolicVelocity", EndDiastolicVelocity),
    ("EndocardialArea", EndocardialArea),
    ("EndometriumThickness", EndometriumThickness),
    ("EnvelopeDuration", EnvelopeDuration),
    ("EpicardialArea", EpicardialArea),
    ("EstimatedLMPByEDD", EstimatedLMPByEDD),
    ("EstimatedWeight", EstimatedWeight),
    ("ExerciseStressTestReport", ExerciseStressTestReport),
    ("FICKCardiacIndex", FICKCardiacIndex),
    ("FICKCardiacOutput", FICKCardiacOutput),
    ("FLByGAASUM2000", FLByGAASUM2000),
    ("FLByGAChitty1994", FLByGAChitty1994),
    ("FLByGAHadlock1984", FLByGAHadlock1984),
    ("FLByGAJeanty1982", FLByGAJeanty1982),
    ("FLByGAMerz1988", FLByGAMerz1988),
    ("FLByGAShinozuka1996", FLByGAShinozuka1996),
    ("FLChitty1997", FLChitty1997),
    ("FLHadlock1984", FLHadlock1984),
    ("FLHansmann1985", FLHansmann1985),
    ("FLHansmann1986", FLHansmann1986),
    ("FLHohler1982", FLHohler1982),
    ("FLJeanty1982", FLJeanty1982),
    ("FLJeanty1984", FLJeanty1984),
    ("FLLessoway1998", FLLessoway1998),
    ("FLMerz1987", FLMerz1987),
    ("FLMerz1988", FLMerz1988),
    ("FLOsaka1989", FLOsaka1989),
    ("FLPerAC", FLPerAC),
    ("FLPerBPD", FLPerBPD),
    ("FLPerHC", FLPerHC),
    ("FLShinozuka1996", FLShinozuka1996),
    ("FLTokyo1986", FLTokyo1986),
    ("FWPByGAAlexander1996", FWPByGAAlexander1996),
    ("FWPByGABrenner1976", FWPByGABrenner1976),
    ("FWPByGAHadlock1985", FWPByGAHadlock1985),
    ("FWPByGAWilliams1982", FWPByGAWilliams1982),
    ("FemaleSingletonBWPByGAArbuckle1993", FemaleSingletonBWPByGAArbuckle1993),
    ("FemaleTwinsBWPByGAArbuckle1993", FemaleTwinsBWPByGAArbuckle1993),
    ("FemurLength", FemurLength),
    ("FetalACAResistivityIndex", FetalACAResistivityIndex),
    ("FetalBreathing", FetalBreathing),
    ("FetalHeartPosition", FetalHeartPosition),
    ("FetalHeartRate", FetalHeartRate),
    ("FetalHeartReactivity", FetalHeartReactivity),
    ("FetalMCAResistivityIndex", FetalMCAResistivityIndex),
    ("FetalTone", FetalTone),
    ("FetalTrunkCrossSectionalAreaOsaka1989", FetalTrunkCrossSectionalAreaOsaka1989),
    ("FetusID", FetusID),
    ("FibulaByGAJeanty1983", FibulaByGAJeanty1983),
    ("FibulaJeanty1983", FibulaJeanty1983),
    ("FibulaLength", FibulaLength),
    ("FibulaMerz1987", FibulaMerz1987),
    ("FillingTime", FillingTime),
    ("Findings", Findings),
    ("FirstQuadrantDiameter", FirstQuadrantDiameter),
    ("FlowArea", FlowArea),
    ("FlowRadius", FlowRadius),
    ("FluoroscopyStudyReport", FluoroscopyStudyReport),
    ("FollicleDiameter", FollicleDiameter),
    ("FootLength", FootLength),
    ("FootLengthMercer1987", FootLengthMercer1987),
    ("ForamenOvaleDiameterPerAorticRootDiameter", ForamenOvaleDiameterPerAorticRootDiameter),
    ("FourthQuadrantDiameter", FourthQuadrantDiameter),
    ("FractionalShortening", FractionalShortening),
    ("GSByGARempen1991", GSByGARempen1991),
    ("GSDaya1991", GSDaya1991),
    ("GSHansmann1979", GSHansmann1979),
    ("GSHansmann1982", GSHansmann1982),
    ("GSHellman1969", GSHellman1969),
    ("GSNyberg1992", GSNyberg1992),
    ("GSRempen1991", GSRempen1991),
    ("GSTokyo1986", GSTokyo1986),
    ("GestationalAge", GestationalAge),
    ("GestationalAgeByLMP", GestationalAgeByLMP),
    ("GestationalAgeByOvulationDate", GestationalAgeByOvulationDate),
    ("GestationalSacDiameter", GestationalSacDiameter),
    ("GlomerularFiltrationRateBlackMDRD", GlomerularFiltrationRateBlackMDRD),
    ("GlomerularFiltrationRateCreatinineBasedFormulaSchwartz", GlomerularFiltrationRateCreatinineBasedFormulaSchwartz),
    ("GlomerularFiltrationRateCystatinBasedFormula", GlomerularFiltrationRateCystatinBasedFormula),
    ("GlomerularFiltrationRateFemaleMDRD", GlomerularFiltrationRateFemaleMDRD),
    ("GlomerularFiltrationRateMDRD", GlomerularFiltrationRateMDRD),
    ("GlomerularFiltrationRateNonBlackMDRD", GlomerularFiltrationRateNonBlackMDRD),
    ("Glucose", Glucose),
    ("Gravida", Gravida),
    ("GrossBodyMovement", GrossBodyMovement),
    ("HCASUM2000", HCASUM2000),
    ("HCByGAASUM2000", HCByGAASUM2000),
    ("HCByGAHadlock1984", HCByGAHadlock1984),
    ("HCByGAJeanty1982", HCByGAJeanty1982),
    ("HCByGAMerz1988", HCByGAMerz1988),
    ("HCDerivedByGAChitty1994", HCDerivedByGAChitty1994),
    ("HCDerivedChitty1997", HCDerivedChitty1997),
    ("HCHadlock1984", HCHadlock1984),
    ("HCHansmann1985", HCHansmann1985),
    ("HCHansmann1986", HCHansmann1986),
    ("HCJeanty1982", HCJeanty1982),
    ("HCJeanty1984", HCJeanty1984),
    ("HCLessoway1998", HCLessoway1998),
    ("HCMeasuredChitty1997", HCMeasuredChitty1997),
    ("HCMerz1988", HCMerz1988),
    ("HCPerAC", HCPerAC),
    ("HCPerACByGACampbell1977", HCPerACByGACampbell1977),
    ("HER2", HER2),
    ("HRCorrectedMeanVelocityOfCircumferentialFiberShortening", HRCorrectedMeanVelocityOfCircumferentialFiberShortening),
    ("HePerHimPerHisPerHisPerHimself", HePerHimPerHisPerHisPerHimself),
    ("HeadCT", HeadCT),
    ("HeadCircumference", HeadCircumference),
    ("HealthStatus", HealthStatus),
    ("HeartRate", HeartRate),
    ("HeartRateCorrectedEjectionTime", HeartRateCorrectedEjectionTime),
    ("HeartRateCorrectedPreEjectionPeriod", HeartRateCorrectedPreEjectionPeriod),
    ("Hematocrit", Hematocrit),
    ("Hemoglobin", Hemoglobin),
    ("HepaticVeinAtrialContractionReversalPeakVelocity", HepaticVeinAtrialContractionReversalPeakVelocity),
    ("HepaticVeinDiastolicPeakVelocity", HepaticVeinDiastolicPeakVelocity),
    ("HepaticVeinSystolicPeakVelocity", HepaticVeinSystolicPeakVelocity),
    ("HepaticVeinSystolicToDiastolicRatio", HepaticVeinSystolicToDiastolicRatio),
    ("History", History),
    ("HistoryOfFamilyMemberDiseases", HistoryOfFamilyMemberDiseases),
    ("HistoryOfMedicalDeviceUse", HistoryOfMedicalDeviceUse),
    ("HistoryOfMedicationUse", HistoryOfMedicationUse),
    ("HolterStudyReport", HolterStudyReport),
    ("HumerusJeanty1984", HumerusJeanty1984),
    ("HumerusLength", HumerusLength),
    ("HumerusLengthASUM2000", HumerusLengthASUM2000),
    ("HumerusLengthByGAASUM2000", HumerusLengthByGAASUM2000),
    ("HumerusLengthOsaka1989", HumerusLengthOsaka1989),
    ("HumerusMerz1987", HumerusMerz1987),
    ("ICAPerCCAVelocityRatio", ICAPerCCAVelocityRatio),
    ("IODMayden1982", IODMayden1982),
    ("IODTrout1994", IODTrout1994),
    ("Impressions", Impressions),
    ("IndicationsForProcedure", IndicationsForProcedure),
    ("InferiorVenaCavaDiameter_18006_7", InferiorVenaCavaDiameter_18006_7),
    ("InferiorVenaCavaDiameter_79967_6", InferiorVenaCavaDiameter_79967_6),
    ("InferiorVenaCavaPercentCollapse", InferiorVenaCavaPercentCollapse),
    ("InnerOrbitalDiameter", InnerOrbitalDiameter),
    ("InterventricularSeptumDiastolicDimension2D", InterventricularSeptumDiastolicDimension2D),
    ("InterventricularSeptumDiastolicDimensionMM", InterventricularSeptumDiastolicDimensionMM),
    ("InterventricularSeptumDiastolicThickness", InterventricularSeptumDiastolicThickness),
    ("InterventricularSeptumPercentThickening", InterventricularSeptumPercentThickening),
    ("InterventricularSeptumSystolicDimension2D", InterventricularSeptumSystolicDimension2D),
    ("InterventricularSeptumSystolicDimensionMM", InterventricularSeptumSystolicDimensionMM),
    ("InterventricularSeptumSystolicThickness", InterventricularSeptumSystolicThickness),
    ("InterventricularSeptumTimeToPeakDisplacement", InterventricularSeptumTimeToPeakDisplacement),
    ("InterventricularSeptumToPosteriorWallThicknessRatio", InterventricularSeptumToPosteriorWallThicknessRatio),
    ("IntravascularArterialMeanPressure", IntravascularArterialMeanPressure),
    ("IntravascularDiastolicBloodPressure", IntravascularDiastolicBloodPressure),
    ("IntravascularSystolicBloodPressure", IntravascularSystolicBloodPressure),
    ("IsovolumicContractionTime", IsovolumicContractionTime),
    ("IsovolumicRelaxationTime", IsovolumicRelaxationTime),
    ("JetArea", JetArea),
    ("KeyImages", KeyImages),
    ("LMP", LMP),
    ("LVBasalAnteriorTimeToSVmaxTsBasalAnterior", LVBasalAnteriorTimeToSVmaxTsBasalAnterior),
    ("LVBasalAnteroseptalTimeToSVmaxTSBasalAnteroseptal", LVBasalAnteroseptalTimeToSVmaxTSBasalAnteroseptal),
    ("LVBasalInferiorTimeToSVmaxTsBasalInferior", LVBasalInferiorTimeToSVmaxTsBasalInferior),
    ("LVBasalLateralTimeToSVmaxTsBasalLateral", LVBasalLateralTimeToSVmaxTsBasalLateral),
    ("LVBasalPosteriorTimeToSVmaxTsBasalPosterior", LVBasalPosteriorTimeToSVmaxTsBasalPosterior),
    ("LVBasalSeptalTimeToSVmaxTsBasalSeptal", LVBasalSeptalTimeToSVmaxTsBasalSeptal),
    ("LVEjectionFraction", LVEjectionFraction),
    ("LVMidAnteriorTimeToSVmaxTsMidAnterior", LVMidAnteriorTimeToSVmaxTsMidAnterior),
    ("LVMidAnteroseptalTimeToSVmaxTsMidAnteroseptal", LVMidAnteroseptalTimeToSVmaxTsMidAnteroseptal),
    ("LVMidInferiorTimeToSVmaxTsMidInferior", LVMidInferiorTimeToSVmaxTsMidInferior),
    ("LVMidLateralTimeToSVmaxTsMidLateral", LVMidLateralTimeToSVmaxTsMidLateral),
    ("LVMidPosteriorTimeToSVmaxTsMidPosterior", LVMidPosteriorTimeToSVmaxTsMidPosterior),
    ("LVMidSeptalTimeToSVmaxTsMidSeptal", LVMidSeptalTimeToSVmaxTsMidSeptal),
    ("LVStrokeVolume", LVStrokeVolume),
    ("LVTsSDDyssynchronyIndex", LVTsSDDyssynchronyIndex),
    ("LVWallMotionSegmentalFindings", LVWallMotionSegmentalFindings),
    ("LateralVentricleWidth", LateralVentricleWidth),
    ("LeafletSeparation", LeafletSeparation),
    ("LeafletThickness", LeafletThickness),
    ("LeftAtrialAppendagePeakVelocity", LeftAtrialAppendagePeakVelocity),
    ("LeftAtrialEndSystolicArea2C", LeftAtrialEndSystolicArea2C),
    ("LeftAtrialEndSystolicArea4C", LeftAtrialEndSystolicArea4C),
    ("LeftAtrialEndSystolicDiameterAP2D", LeftAtrialEndSystolicDiameterAP2D),
    ("LeftAtrialEndSystolicDiameterAP2DPerBSA", LeftAtrialEndSystolicDiameterAP2DPerBSA),
    ("LeftAtrialEndSystolicDiameterAPMM", LeftAtrialEndSystolicDiameterAPMM),
    ("LeftAtrialEndSystolicDiameterAPMMPerBSA", LeftAtrialEndSystolicDiameterAPMMPerBSA),
    ("LeftAtrialEndSystolicLength2C", LeftAtrialEndSystolicLength2C),
    ("LeftAtrialEndSystolicLength4C", LeftAtrialEndSystolicLength4C),
    ("LeftAtrialEndSystolicVolumeBiplaneAreaLength", LeftAtrialEndSystolicVolumeBiplaneAreaLength),
    ("LeftAtrialEndSystolicVolumeBiplaneAreaLengthPerBSA", LeftAtrialEndSystolicVolumeBiplaneAreaLengthPerBSA),
    ("LeftAtrialEndSystolicVolumeBiplaneMOD", LeftAtrialEndSystolicVolumeBiplaneMOD),
    ("LeftAtrialEndSystolicVolumeBiplaneMODPerBSA", LeftAtrialEndSystolicVolumeBiplaneMODPerBSA),
    ("LeftAtrialEndSystolicVolumeSinglePlane2CMOD", LeftAtrialEndSystolicVolumeSinglePlane2CMOD),
    ("LeftAtrialEndSystolicVolumeSinglePlane4CMOD", LeftAtrialEndSystolicVolumeSinglePlane4CMOD),
    ("LeftAtriumAnteroPosteriorSystolicDimension", LeftAtriumAnteroPosteriorSystolicDimension),
    ("LeftAtriumAreaA4CView", LeftAtriumAreaA4CView),
    ("LeftAtriumToAorticRootRatio", LeftAtriumToAorticRootRatio),
    ("LeftAtriumVolumeToRightAtriumVolumeRatio", LeftAtriumVolumeToRightAtriumVolumeRatio),
    ("LeftFetalEarLength", LeftFetalEarLength),
    ("LeftKidneyLength", LeftKidneyLength),
    ("LeftKidneyThickness", LeftKidneyThickness),
    ("LeftKidneyWidth", LeftKidneyWidth),
    ("LeftOvaryHeight", LeftOvaryHeight),
    ("LeftOvaryLength", LeftOvaryLength),
    ("LeftOvaryVolume", LeftOvaryVolume),
    ("LeftOvaryWidth", LeftOvaryWidth),
    ("LeftPulmonaryArteryDiameter_18019_0", LeftPulmonaryArteryDiameter_18019_0),
    ("LeftPulmonaryArteryDiameter_79987_4", LeftPulmonaryArteryDiameter_79987_4),
    ("LeftVenticularPosteriorWallTimeToPeakDisplacement", LeftVenticularPosteriorWallTimeToPeakDisplacement),
    ("LeftVentricleCardiacOutput", LeftVentricleCardiacOutput),
    ("LeftVentricleDiastolicMajorAxis", LeftVentricleDiastolicMajorAxis),
    ("LeftVentricleEToATissueVelocityRatio", LeftVentricleEToATissueVelocityRatio),
    ("LeftVentricleInternalEndDiastolicDimension", LeftVentricleInternalEndDiastolicDimension),
    ("LeftVentricleInternalSystolicDimension", LeftVentricleInternalSystolicDimension),
    ("LeftVentricleMass", LeftVentricleMass),
    ("LeftVentricleMeridionalWallStress", LeftVentricleMeridionalWallStress),
    ("LeftVentriclePerRightVentricleDiameterRatio", LeftVentriclePerRightVentricleDiameterRatio),
    ("LeftVentriclePosteriorWallDiastolicThickness", LeftVentriclePosteriorWallDiastolicThickness),
    ("LeftVentriclePosteriorWallPercentThickening", LeftVentriclePosteriorWallPercentThickening),
    ("LeftVentriclePosteriorWallSystolicThickness", LeftVentriclePosteriorWallSystolicThickness),
    ("LeftVentricleSystolicMajorAxis", LeftVentricleSystolicMajorAxis),
    ("LeftVentricularCardiacOutput", LeftVentricularCardiacOutput),
    ("LeftVentricularCardiacOutput2CAreaLength", LeftVentricularCardiacOutput2CAreaLength),
    ("LeftVentricularCardiacOutput2CMOD", LeftVentricularCardiacOutput2CMOD),
    ("LeftVentricularCardiacOutput3D", LeftVentricularCardiacOutput3D),
    ("LeftVentricularCardiacOutput4CAreaLength", LeftVentricularCardiacOutput4CAreaLength),
    ("LeftVentricularCardiacOutput4CMOD", LeftVentricularCardiacOutput4CMOD),
    ("LeftVentricularCardiacOutputBiplaneAreaLength", LeftVentricularCardiacOutputBiplaneAreaLength),
    ("LeftVentricularCardiacOutputBiplaneEllipse", LeftVentricularCardiacOutputBiplaneEllipse),
    ("LeftVentricularCardiacOutputBullet", LeftVentricularCardiacOutputBullet),
    ("LeftVentricularCardiacOutputCalculated", LeftVentricularCardiacOutputCalculated),
    ("LeftVentricularCardiacOutputCube", LeftVentricularCardiacOutputCube),
    ("LeftVentricularCardiacOutputCubed", LeftVentricularCardiacOutputCubed),
    ("LeftVentricularCardiacOutputGibson", LeftVentricularCardiacOutputGibson),
    ("LeftVentricularCardiacOutputLVOT", LeftVentricularCardiacOutputLVOT),
    ("LeftVentricularCardiacOutputMModeCalculated", LeftVentricularCardiacOutputMModeCalculated),
    ("LeftVentricularCardiacOutputMModeCube", LeftVentricularCardiacOutputMModeCube),
    ("LeftVentricularCardiacOutputMModeGibson", LeftVentricularCardiacOutputMModeGibson),
    ("LeftVentricularCardiacOutputMModeTeichholz", LeftVentricularCardiacOutputMModeTeichholz),
    ("LeftVentricularCardiacOutputModifiedBiplane", LeftVentricularCardiacOutputModifiedBiplane),
    ("LeftVentricularCardiacOutputSinglePlaneEllipse", LeftVentricularCardiacOutputSinglePlaneEllipse),
    ("LeftVentricularCardiacOutputTeichholz", LeftVentricularCardiacOutputTeichholz),
    ("LeftVentricularEDVolume", LeftVentricularEDVolume),
    ("LeftVentricularESVolume", LeftVentricularESVolume),
    ("LeftVentricularEjectionFraction3D", LeftVentricularEjectionFraction3D),
    ("LeftVentricularEjectionFractionBiplaneMOD", LeftVentricularEjectionFractionBiplaneMOD),
    ("LeftVentricularEjectionFractionByAngiography", LeftVentricularEjectionFractionByAngiography),
    ("LeftVentricularEjectionFractionByCT", LeftVentricularEjectionFractionByCT),
    ("LeftVentricularEjectionFractionByMR", LeftVentricularEjectionFractionByMR),
    ("LeftVentricularEjectionFractionByUS", LeftVentricularEjectionFractionByUS),
    ("LeftVentricularEjectionFractionSinglePlane2CMOD", LeftVentricularEjectionFractionSinglePlane2CMOD),
    ("LeftVentricularEjectionFractionSinglePlane4CMOD", LeftVentricularEjectionFractionSinglePlane4CMOD),
    ("LeftVentricularEjectionFractionTeichholz2D", LeftVentricularEjectionFractionTeichholz2D),
    ("LeftVentricularEjectionFractionTeichholzMM", LeftVentricularEjectionFractionTeichholzMM),
    ("LeftVentricularEndDiastolicLength4C", LeftVentricularEndDiastolicLength4C),
    ("LeftVentricularEndDiastolicVolume", LeftVentricularEndDiastolicVolume),
    ("LeftVentricularEndDiastolicVolume3D", LeftVentricularEndDiastolicVolume3D),
    ("LeftVentricularEndDiastolicVolumeBiplaneMOD", LeftVentricularEndDiastolicVolumeBiplaneMOD),
    ("LeftVentricularEndDiastolicVolumeBiplaneMODPerBSA", LeftVentricularEndDiastolicVolumeBiplaneMODPerBSA),
    ("LeftVentricularEndDiastolicVolumeSinglePlane2CMOD", LeftVentricularEndDiastolicVolumeSinglePlane2CMOD),
    ("LeftVentricularEndDiastolicVolumeSinglePlane4CMOD", LeftVentricularEndDiastolicVolumeSinglePlane4CMOD),
    ("LeftVentricularEndSystolicVolume", LeftVentricularEndSystolicVolume),
    ("LeftVentricularEndSystolicVolume3D", LeftVentricularEndSystolicVolume3D),
    ("LeftVentricularEndSystolicVolumeBiplaneMOD", LeftVentricularEndSystolicVolumeBiplaneMOD),
    ("LeftVentricularEndSystolicVolumeBiplaneMODPerBSA", LeftVentricularEndSystolicVolumeBiplaneMODPerBSA),
    ("LeftVentricularEndSystolicVolumeSinglePlane2CMOD", LeftVentricularEndSystolicVolumeSinglePlane2CMOD),
    ("LeftVentricularEndSystolicVolumeSinglePlane4CMOD", LeftVentricularEndSystolicVolumeSinglePlane4CMOD),
    ("LeftVentricularEndocardialAreaSAXPMLevel", LeftVentricularEndocardialAreaSAXPMLevel),
    ("LeftVentricularEpicardialAreaSAXPMLevel", LeftVentricularEpicardialAreaSAXPMLevel),
    ("LeftVentricularFractionalShortening", LeftVentricularFractionalShortening),
    ("LeftVentricularFractionalShorteningOfMinorAxis2D", LeftVentricularFractionalShorteningOfMinorAxis2D),
    ("LeftVentricularFractionalShorteningOfMinorAxisMM", LeftVentricularFractionalShorteningOfMinorAxisMM),
    ("LeftVentricularInternalDiastolicDimension2D", LeftVentricularInternalDiastolicDimension2D),
    ("LeftVentricularInternalDiastolicDimensionMM", LeftVentricularInternalDiastolicDimensionMM),
    ("LeftVentricularInternalDiastolicDimensionPerBSA_80009_4", LeftVentricularInternalDiastolicDimensionPerBSA_80009_4),
    ("LeftVentricularInternalDiastolicDimensionPerBSA_80010_2", LeftVentricularInternalDiastolicDimensionPerBSA_80010_2),
    ("LeftVentricularInternalSystolicDimension2D", LeftVentricularInternalSystolicDimension2D),
    ("LeftVentricularInternalSystolicDimensionMM", LeftVentricularInternalSystolicDimensionMM),
    ("LeftVentricularInternalSystolicDimensionPerBSA_80013_6", LeftVentricularInternalSystolicDimensionPerBSA_80013_6),
    ("LeftVentricularInternalSystolicDimensionPerBSA_80014_4", LeftVentricularInternalSystolicDimensionPerBSA_80014_4),
    ("LeftVentricularIsovolumicRelaxationTime", LeftVentricularIsovolumicRelaxationTime),
    ("LeftVentricularIsovolumicRelaxationTimeByTDI", LeftVentricularIsovolumicRelaxationTimeByTDI),
    ("LeftVentricularMassAreaLength", LeftVentricularMassAreaLength),
    ("LeftVentricularMassAreaLengthPerBSA", LeftVentricularMassAreaLengthPerBSA),
    ("LeftVentricularMassAreaLengthPerHeight27", LeftVentricularMassAreaLengthPerHeight27),
    ("LeftVentricularMassDimensionMethod2D", LeftVentricularMassDimensionMethod2D),
    ("LeftVentricularMassDimensionMethod2DPerBSA", LeftVentricularMassDimensionMethod2DPerBSA),
    ("LeftVentricularMassDimensionMethod2DPerHeight27", LeftVentricularMassDimensionMethod2DPerHeight27),
    ("LeftVentricularMassDimensionMethodMM", LeftVentricularMassDimensionMethodMM),
    ("LeftVentricularMassDimensionMethodMMPerBSA", LeftVentricularMassDimensionMethodMMPerBSA),
    ("LeftVentricularMassDimensionMethodMMPerHeight27", LeftVentricularMassDimensionMethodMMPerHeight27),
    ("LeftVentricularMassTruncatedEllipse", LeftVentricularMassTruncatedEllipse),
    ("LeftVentricularMassTruncatedEllipsePerBSA", LeftVentricularMassTruncatedEllipsePerBSA),
    ("LeftVentricularMassTruncatedEllipsePerHeight27", LeftVentricularMassTruncatedEllipsePerHeight27),
    ("LeftVentricularOutflowTractDimension2D", LeftVentricularOutflowTractDimension2D),
    ("LeftVentricularOutflowTractPerAorticValveVTI", LeftVentricularOutflowTractPerAorticValveVTI),
    ("LeftVentricularOutflowTractVTI", LeftVentricularOutflowTractVTI),
    ("LeftVentricularOutflowTractVmax", LeftVentricularOutflowTractVmax),
    ("LeftVentricularPosteriorWallDiastolicThickness_80031_8", LeftVentricularPosteriorWallDiastolicThickness_80031_8),
    ("LeftVentricularPosteriorWallDiastolicThickness_80032_6", LeftVentricularPosteriorWallDiastolicThickness_80032_6),
    ("LeftVentricularPosteriorWallSystolicThickness_80033_4", LeftVentricularPosteriorWallSystolicThickness_80033_4),
    ("LeftVentricularPosteriorWallSystolicThickness_80034_2", LeftVentricularPosteriorWallSystolicThickness_80034_2),
    ("LeftVentricularPreEjectionPeriod", LeftVentricularPreEjectionPeriod),
    ("LeftVentricularSphericityIndexEndDiastole", LeftVentricularSphericityIndexEndDiastole),
    ("LeftVentricularStrokeVolume3D", LeftVentricularStrokeVolume3D),
    ("LeftVentricularStrokeVolumeAorticRootCalculated", LeftVentricularStrokeVolumeAorticRootCalculated),
    ("LeftVentricularStrokeWork", LeftVentricularStrokeWork),
    ("LeftVentricularStrokeWorkIndex", LeftVentricularStrokeWorkIndex),
    ("LengthOfVertebraTokyo1986", LengthOfVertebraTokyo1986),
    ("LiveBirths", LiveBirths),
    ("MCAPulsatilityIndex", MCAPulsatilityIndex),
    ("MR3DCAMModel", MR3DCAMModel),
    ("MRISpineReport", MRISpineReport),
    ("MRIUnspecifiedBodyRegion", MRIUnspecifiedBodyRegion),
    ("MRIWholeBody", MRIWholeBody),
    ("MacularGridCenterPointThickness", MacularGridCenterPointThickness),
    ("MacularGridCenterSubfieldThickness", MacularGridCenterSubfieldThickness),
    ("MacularGridInnerInferiorSubfieldThickness", MacularGridInnerInferiorSubfieldThickness),
    ("MacularGridInnerNasalSubfieldThickness", MacularGridInnerNasalSubfieldThickness),
    ("MacularGridInnerSuperiorSubfieldThickness", MacularGridInnerSuperiorSubfieldThickness),
    ("MacularGridInnerTemporalSubfieldThickness", MacularGridInnerTemporalSubfieldThickness),
    ("MacularGridOuterInferiorSubfieldThickness", MacularGridOuterInferiorSubfieldThickness),
    ("MacularGridOuterNasalSubfieldThickness", MacularGridOuterNasalSubfieldThickness),
    ("MacularGridOuterSuperiorSubfieldThickness", MacularGridOuterSuperiorSubfieldThickness),
    ("MacularGridOuterTemporalSubfieldThickness", MacularGridOuterTemporalSubfieldThickness),
    ("MacularGridTotalVolume", MacularGridTotalVolume),
    ("MainPulmonaryArteryDiameter_18020_8", MainPulmonaryArteryDiameter_18020_8),
    ("MainPulmonaryArteryDiameter_80049_0", MainPulmonaryArteryDiameter_80049_0),
    ("MainPulmonaryArterySWaveVmax", MainPulmonaryArterySWaveVmax),
    ("MaleSingletonBWPByGAArbuckle1993", MaleSingletonBWPByGAArbuckle1993),
    ("MaleTwinsBWPByGAArbuckle1993", MaleTwinsBWPByGAArbuckle1993),
    ("MammographyScreeningReport", MammographyScreeningReport),
    ("MeanGradient", MeanGradient),
    ("MeanVelocityOfCircumferentialFiberShorteningMeanVcFv", MeanVelocityOfCircumferentialFiberShorteningMeanVcFv),
    ("MedicationsAdministered", MedicationsAdministered),
    ("MinimumDiastolicVelocity", MinimumDiastolicVelocity),
    ("MitralAnnulusDiastolicDiameterA2C", MitralAnnulusDiastolicDiameterA2C),
    ("MitralAnnulusDiastolicDiameterA4C", MitralAnnulusDiastolicDiameterA4C),
    ("MitralAnnulusDiastolicDiameterPLAX", MitralAnnulusDiastolicDiameterPLAX),
    ("MitralAnnulusVTI", MitralAnnulusVTI),
    ("MitralLateralAPeakVelocity", MitralLateralAPeakVelocity),
    ("MitralLateralEPeakVelocity", MitralLateralEPeakVelocity),
    ("MitralLateralEPerERatio", MitralLateralEPerERatio),
    ("MitralLateralEPrimeVmax", MitralLateralEPrimeVmax),
    ("MitralLateralSPeakVelocity", MitralLateralSPeakVelocity),
    ("MitralRegurgitantFlowPISA", MitralRegurgitantFlowPISA),
    ("MitralRegurgitantFractionContinuityVTI", MitralRegurgitantFractionContinuityVTI),
    ("MitralRegurgitantFractionPISA", MitralRegurgitantFractionPISA),
    ("MitralRegurgitationDPPerDtDerivedFromMitralRegVelocity", MitralRegurgitationDPPerDtDerivedFromMitralRegVelocity),
    ("MitralRegurgitationPISARadius", MitralRegurgitationPISARadius),
    ("MitralRegurgitationPISAVelocity", MitralRegurgitationPISAVelocity),
    ("MitralRegurgitationPeakGradient", MitralRegurgitationPeakGradient),
    ("MitralRegurgitationVenaContractaWidth", MitralRegurgitationVenaContractaWidth),
    ("MitralRegurgitationVmax", MitralRegurgitationVmax),
    ("MitralRegurgitationVolumeContinuityVTI", MitralRegurgitationVolumeContinuityVTI),
    ("MitralRegurgitationVolumePISA", MitralRegurgitationVolumePISA),
    ("MitralSeptalAPeakVelocity", MitralSeptalAPeakVelocity),
    ("MitralSeptalEPeakVelocity", MitralSeptalEPeakVelocity),
    ("MitralSeptalEPerERatio", MitralSeptalEPerERatio),
    ("MitralSeptalEPrimeVmax", MitralSeptalEPrimeVmax),
    ("MitralSeptalSPeakVelocity", MitralSeptalSPeakVelocity),
    ("MitralValveAWaveDuration", MitralValveAWaveDuration),
    ("MitralValveAWavePeakVelocity", MitralValveAWavePeakVelocity),
    ("MitralValveAWaveVmax", MitralValveAWaveVmax),
    ("MitralValveAnnulusArea", MitralValveAnnulusArea),
    ("MitralValveAnnulusDiameter", MitralValveAnnulusDiameter),
    ("MitralValveAreaPISA", MitralValveAreaPISA),
    ("MitralValveAreaPlanimetry", MitralValveAreaPlanimetry),
    ("MitralValveAreaPressureHalfTime", MitralValveAreaPressureHalfTime),
    ("MitralValveDecelerationTime", MitralValveDecelerationTime),
    ("MitralValveDiastolicPeakInstantaneousGradient", MitralValveDiastolicPeakInstantaneousGradient),
    ("MitralValveEFSlopeByMMode", MitralValveEFSlopeByMMode),
    ("MitralValveEPSSEWave", MitralValveEPSSEWave),
    ("MitralValveESeptalSeparation", MitralValveESeptalSeparation),
    ("MitralValveEToARatio", MitralValveEToARatio),
    ("MitralValveEWavePeakVelocity", MitralValveEWavePeakVelocity),
    ("MitralValveEWaveVmax", MitralValveEWaveVmax),
    ("MitralValveEffectiveRegurgitantOrificeAreaPISA_29448_8", MitralValveEffectiveRegurgitantOrificeAreaPISA_29448_8),
    ("MitralValveEffectiveRegurgitantOrificeAreaPISA_80071_4", MitralValveEffectiveRegurgitantOrificeAreaPISA_80071_4),
    ("MitralValveEffectiveRegurgitantOrificeAreaVolumetric", MitralValveEffectiveRegurgitantOrificeAreaVolumetric),
    ("MitralValveFlowPropagationVelocityVp", MitralValveFlowPropagationVelocityVp),
    ("MitralValveMeanGradient", MitralValveMeanGradient),
    ("MitralValvePeakInstantaneousGradient", MitralValvePeakInstantaneousGradient),
    ("MitralValvePressureHalfTime", MitralValvePressureHalfTime),
    ("MitralValveVTI", MitralValveVTI),
    ("MitralValveVenaContractaDiameter", MitralValveVenaContractaDiameter),
    ("MitralValveVmax", MitralValveVmax),
    ("MyocardialPerformanceIndexTei", MyocardialPerformanceIndexTei),
    ("NMUnspecifiedBodyRegion", NMUnspecifiedBodyRegion),
    ("NuchalFoldObservation", NuchalFoldObservation),
    ("NuchalFoldThickness", NuchalFoldThickness),
    ("NuchalTranslucency", NuchalTranslucency),
    ("NuclearMedicineThyroidScanReport", NuclearMedicineThyroidScanReport),
    ("NumberOfFetuses", NumberOfFetuses),
    ("NumberOfFetusesByUS", NumberOfFetusesByUS),
    ("NumberOfFolliclesInLeftOvary", NumberOfFolliclesInLeftOvary),
    ("NumberOfFolliclesInRightOvary", NumberOfFolliclesInRightOvary),
    ("OFDASUM2000", OFDASUM2000),
    ("OFDByGAASUM2000", OFDByGAASUM2000),
    ("OFDByGAChitty1994", OFDByGAChitty1994),
    ("OFDHansmann1985", OFDHansmann1985),
    ("OFDHansmann1986", OFDHansmann1986),
    ("OFDLessoway1998", OFDLessoway1998),
    ("OODMayden1982", OODMayden1982),
    ("OODTrout1994", OODTrout1994),
    ("OccipitalFrontalDiameter", OccipitalFrontalDiameter),
    ("OuterOrbitalDiameter", OuterOrbitalDiameter),
    ("OvulationDate", OvulationDate),
    ("OxygenPartialPressureAt50PercentSaturationP50", OxygenPartialPressureAt50PercentSaturationP50),
    ("PETUnspecifiedBodyRegion", PETUnspecifiedBodyRegion),
    ("PETWholeBody", PETWholeBody),
    ("Para", Para),
    ("PastSurgicalHistory", PastSurgicalHistory),
    ("PatientHeight", PatientHeight),
    ("PatientPresentation", PatientPresentation),
    ("PatientWeight", PatientWeight),
    ("PeakBloodVelocity", PeakBloodVelocity),
    ("PeakEWavePerPeakAWaveByUS", PeakEWavePerPeakAWaveByUS),
    ("PeakGradient", PeakGradient),
    ("PeakInstantaneousFlowRate", PeakInstantaneousFlowRate),
    ("PeakReversalVelocityDuringAtrialContraction", PeakReversalVelocityDuringAtrialContraction),
    ("PeakSystolicVelocity", PeakSystolicVelocity),
    ("PeakTissueVelocity", PeakTissueVelocity),
    ("PercentThickening", PercentThickening),
    ("PosteriorHornLateralVentricularWidth", PosteriorHornLateralVentricularWidth),
    ("PreEjectionPeriod", PreEjectionPeriod),
    ("PreEjectionPeriodPerEjectionTimeRatio", PreEjectionPeriodPerEjectionTimeRatio),
    ("PressureHalfTime", PressureHalfTime),
    ("Preterm", Preterm),
    ("PreviousFindings", PreviousFindings),
    ("PriorProcedureDescriptions", PriorProcedureDescriptions),
    ("ProblemList", ProblemList),
    ("ProstateCancerAntigen", ProstateCancerAntigen),
    ("ProstateSpecificAntigenDensity", ProstateSpecificAntigenDensity),
    ("PulmVeinADurationToMVADurationDifference", PulmVeinADurationToMVADurationDifference),
    ("PulmonaryArteryMainOxygenContent", PulmonaryArteryMainOxygenContent),
    ("PulmonaryArteryPressureUsingAccelTime", PulmonaryArteryPressureUsingAccelTime),
    ("PulmonaryToSystemicShuntFlowRatio", PulmonaryToSystemicShuntFlowRatio),
    ("PulmonaryVeinADurationMitralValveADurationRatio", PulmonaryVeinADurationMitralValveADurationRatio),
    ("PulmonaryVeinAVTIToMitralValveVTIRatio", PulmonaryVeinAVTIToMitralValveVTIRatio),
    ("PulmonaryVeinAWaveDuration", PulmonaryVeinAWaveDuration),
    ("PulmonaryVeinAWaveVmax", PulmonaryVeinAWaveVmax),
    ("PulmonaryVeinAtrialContractionReversalPeakVelocity", PulmonaryVeinAtrialContractionReversalPeakVelocity),
    ("PulmonaryVeinDWaveVmax", PulmonaryVeinDWaveVmax),
    ("PulmonaryVeinDiastolicPeakVelocity", PulmonaryVeinDiastolicPeakVelocity),
    ("PulmonaryVeinSWaveVmax", PulmonaryVeinSWaveVmax),
    ("PulmonaryVeinSystolicPeakVelocity", PulmonaryVeinSystolicPeakVelocity),
    ("PulmonaryVeinSystolicToDiastolicRatio", PulmonaryVeinSystolicToDiastolicRatio),
    ("PulmonaryWedgeOxygenContent", PulmonaryWedgeOxygenContent),
    ("PulmonicAnnulusDiameter", PulmonicAnnulusDiameter),
    ("PulmonicRegurgitationEndDiastolicPeakGradient", PulmonicRegurgitationEndDiastolicPeakGradient),
    ("PulmonicRegurgitationEndDiastolicVelocity", PulmonicRegurgitationEndDiastolicVelocity),
    ("PulmonicRegurgitationVmax", PulmonicRegurgitationVmax),
    ("PulmonicValveAccelerationTime", PulmonicValveAccelerationTime),
    ("PulmonicValveAreaByContinuity", PulmonicValveAreaByContinuity),
    ("PulmonicValveEjectionTime", PulmonicValveEjectionTime),
    ("PulmonicValvePeakGradient", PulmonicValvePeakGradient),
    ("PulmonicValveVTI", PulmonicValveVTI),
    ("PulmonicValveVmax", PulmonicValveVmax),
    ("PulsatilityIndex", PulsatilityIndex),
    ("ROIInternalDimensionByUS", ROIInternalDimensionByUS),
    ("ROIThicknessByUS", ROIThicknessByUS),
    ("RVEjectionFraction", RVEjectionFraction),
    ("RVStrokeVolume", RVStrokeVolume),
    ("RadiationExposureAndProtectionInformation", RadiationExposureAndProtectionInformation),
    ("RadiologyReport", RadiologyReport),
    ("RadionuclideBrainScan", RadionuclideBrainScan),
    ("RadionuclideScanOfWholeBody", RadionuclideScanOfWholeBody),
    ("RadiusByGAJeanty1983", RadiusByGAJeanty1983),
    ("RadiusJeanty1983", RadiusJeanty1983),
    ("RadiusLength", RadiusLength),
    ("RadiusMerz1987", RadiusMerz1987),
    ("Recommendations", Recommendations),
    ("RelevantDiagnosticTestsAndPerOrLaboratoryData", RelevantDiagnosticTestsAndPerOrLaboratoryData),
    ("RenalArteryPerAortaVelocityRatio", RenalArteryPerAortaVelocityRatio),
    ("Request", Request),
    ("ResistivityIndex", ResistivityIndex),
    ("RespirationRhythm", RespirationRhythm),
    ("RestingHeartRate", RestingHeartRate),
    ("RightAtrialEndSystolicArea4C", RightAtrialEndSystolicArea4C),
    ("RightAtrialMajorAxisDimension4C", RightAtrialMajorAxisDimension4C),
    ("RightAtrialMinorAxisDimension4C", RightAtrialMinorAxisDimension4C),
    ("RightAtrialMinorAxisDimension4CPerBSA", RightAtrialMinorAxisDimension4CPerBSA),
    ("RightAtriumAreaA4CView", RightAtriumAreaA4CView),
    ("RightAtriumSystolicPressure", RightAtriumSystolicPressure),
    ("RightFetalEarLength", RightFetalEarLength),
    ("RightKidneyLength", RightKidneyLength),
    ("RightKidneyThickness", RightKidneyThickness),
    ("RightKidneyWidth", RightKidneyWidth),
    ("RightOvaryHeight", RightOvaryHeight),
    ("RightOvaryLength", RightOvaryLength),
    ("RightOvaryVolume", RightOvaryVolume),
    ("RightOvaryWidth", RightOvaryWidth),
    ("RightPulmonaryArteryDiameter_18021_6", RightPulmonaryArteryDiameter_18021_6),
    ("RightPulmonaryArteryDiameter_80079_7", RightPulmonaryArteryDiameter_80079_7),
    ("RightVentricularAnteriorWallDiastolicThickness", RightVentricularAnteriorWallDiastolicThickness),
    ("RightVentricularAnteriorWallSystolicThickness", RightVentricularAnteriorWallSystolicThickness),
    ("RightVentricularBasalDimension4C", RightVentricularBasalDimension4C),
    ("RightVentricularEDVolume", RightVentricularEDVolume),
    ("RightVentricularESVolume", RightVentricularESVolume),
    ("RightVentricularEjectionFractionByAngiography", RightVentricularEjectionFractionByAngiography),
    ("RightVentricularEjectionFractionByCT", RightVentricularEjectionFractionByCT),
    ("RightVentricularEjectionFractionByMR", RightVentricularEjectionFractionByMR),
    ("RightVentricularEjectionTime", RightVentricularEjectionTime),
    ("RightVentricularEndDiastolicArea4C", RightVentricularEndDiastolicArea4C),
    ("RightVentricularEndSystolicArea4C", RightVentricularEndSystolicArea4C),
    ("RightVentricularFractionalAreaChange", RightVentricularFractionalAreaChange),
    ("RightVentricularFreeWallThickness2D", RightVentricularFreeWallThickness2D),
    ("RightVentricularFreeWallThicknessMM", RightVentricularFreeWallThicknessMM),
    ("RightVentricularInternalDiastolicDimension", RightVentricularInternalDiastolicDimension),
    ("RightVentricularInternalSystolicDimension", RightVentricularInternalSystolicDimension),
    ("RightVentricularIntrachamberSystolicPressure", RightVentricularIntrachamberSystolicPressure),
    ("RightVentricularMidCavityDimension4C", RightVentricularMidCavityDimension4C),
    ("RightVentricularMyocardialPerformanceIndex", RightVentricularMyocardialPerformanceIndex),
    ("RightVentricularOutflowTractDiameterAtPulmonicValveRVOTDistal", RightVentricularOutflowTractDiameterAtPulmonicValveRVOTDistal),
    ("RightVentricularOutflowTractDiameterAtSubvalvularLevelRVOTProximal", RightVentricularOutflowTractDiameterAtSubvalvularLevelRVOTProximal),
    ("RightVentricularOutflowTractVTI", RightVentricularOutflowTractVTI),
    ("RightVentricularPreEjectionPeriod", RightVentricularPreEjectionPeriod),
    ("RightVentricularStrokeWork", RightVentricularStrokeWork),
    ("RightVentricularStrokeWorkIndex", RightVentricularStrokeWorkIndex),
    ("SPECTBrain", SPECTBrain),
    ("SecondQuadrantDiameter", SecondQuadrantDiameter),
    ("SerumCreatinine", SerumCreatinine),
    ("ShePerHerPerHerPerHersPerHerself", ShePerHerPerHerPerHersPerHerself),
    ("SocialHistory", SocialHistory),
    ("SpineLength", SpineLength),
    ("SpineLengthTokyo1989", SpineLengthTokyo1989),
    ("Stage", Stage),
    ("Status", Status),
    ("StenosisPeakGradient", StenosisPeakGradient),
    ("StenosisPeakVelocity", StenosisPeakVelocity),
    ("Summary", Summary),
    ("SystolicToDiastolicVelocityRatio", SystolicToDiastolicVelocityRatio),
    ("TADEriksen1985", TADEriksen1985),
    ("TADHansmann1979", TADHansmann1979),
    ("TADTokyo1986", TADTokyo1986),
    ("TCDByGAGoldstein1987", TCDByGAGoldstein1987),
    ("TCDChitty1994", TCDChitty1994),
    ("TCDGoldstein1987", TCDGoldstein1987),
    ("TCDHill1990", TCDHill1990),
    ("TCDNimrod1986", TCDNimrod1986),
    ("Term", Term),
    ("ThCChitkara1987", ThCChitkara1987),
    ("TheyPerThemPerTheirPerTheirsPerThemselves", TheyPerThemPerTheirPerTheirsPerThemselves),
    ("ThirdQuadrantDiameter", ThirdQuadrantDiameter),
    ("ThoracicAortaCoarctationSystolicPeakInstantaneousGradient", ThoracicAortaCoarctationSystolicPeakInstantaneousGradient),
    ("ThoracicAortaCoarctationSystolicPeakVelocity", ThoracicAortaCoarctationSystolicPeakVelocity),
    ("ThoracicArea", ThoracicArea),
    ("ThoracicCircumference", ThoracicCircumference),
    ("TibiaJeanty1984", TibiaJeanty1984),
    ("TibiaLength", TibiaLength),
    ("TibialPerBrachialIndex", TibialPerBrachialIndex),
    ("TimeAveragedMeanVelocity", TimeAveragedMeanVelocity),
    ("TimeAveragedPeakVelocity", TimeAveragedPeakVelocity),
    ("TimeFromQWaveToPulmonicValveCloses", TimeFromQWaveToPulmonicValveCloses),
    ("TimeFromQWaveToTricuspidValveOpens", TimeFromQWaveToTricuspidValveOpens),
    ("TimeToLeftVentricleETissueVelocity", TimeToLeftVentricleETissueVelocity),
    ("TimeToLeftVentricleSTissueVelocity", TimeToLeftVentricleSTissueVelocity),
    ("TimeToPeakByUS", TimeToPeakByUS),
    ("TissueVelocityTimeIntegralVTIForTheAreaUnderLeftVentricleAWave", TissueVelocityTimeIntegralVTIForTheAreaUnderLeftVentricleAWave),
    ("TissueVelocityTimeIntegralVTIForTheAreaUnderLeftVentricleEWave", TissueVelocityTimeIntegralVTIForTheAreaUnderLeftVentricleEWave),
    ("TransverseAbdominalDiameter", TransverseAbdominalDiameter),
    ("TransverseCerebellarDiameter", TransverseCerebellarDiameter),
    ("TransverseThoracicDiameter", TransverseThoracicDiameter),
    ("TransverseThoracicDiameterHansmann1985", TransverseThoracicDiameterHansmann1985),
    ("TransverseThoracicDiameterLessoway1998", TransverseThoracicDiameterLessoway1998),
    ("TricuspidAnnularPlaneSystolicExcursionTAPSE", TricuspidAnnularPlaneSystolicExcursionTAPSE),
    ("TricuspidAnnulusDiameter", TricuspidAnnulusDiameter),
    ("TricuspidRegurgitationDPPerDt", TricuspidRegurgitationDPPerDt),
    ("TricuspidRegurgitationPISARadius", TricuspidRegurgitationPISARadius),
    ("TricuspidRegurgitationPeakGradient", TricuspidRegurgitationPeakGradient),
    ("TricuspidRegurgitationVenaContractaWidth", TricuspidRegurgitationVenaContractaWidth),
    ("TricuspidRegurgitationVmax", TricuspidRegurgitationVmax),
    ("TricuspidValveAPrimeVmax", TricuspidValveAPrimeVmax),
    ("TricuspidValveAWavePeakVelocity", TricuspidValveAWavePeakVelocity),
    ("TricuspidValveAWaveVmax", TricuspidValveAWaveVmax),
    ("TricuspidValveAnnulusDiameter", TricuspidValveAnnulusDiameter),
    ("TricuspidValveAnnulusRegionCrossectionArea", TricuspidValveAnnulusRegionCrossectionArea),
    ("TricuspidValveClosureToOpeningTime", TricuspidValveClosureToOpeningTime),
    ("TricuspidValveDecelerationTime", TricuspidValveDecelerationTime),
    ("TricuspidValveDiastolicVTI", TricuspidValveDiastolicVTI),
    ("TricuspidValveEPrimeVmax", TricuspidValveEPrimeVmax),
    ("TricuspidValveEToARatio", TricuspidValveEToARatio),
    ("TricuspidValveEWavePeakVelocity", TricuspidValveEWavePeakVelocity),
    ("TricuspidValveEWaveVmax", TricuspidValveEWaveVmax),
    ("TricuspidValveEffectiveRegurgitantOrificeAreaPISA", TricuspidValveEffectiveRegurgitantOrificeAreaPISA),
    ("TricuspidValveEffectiveRegurgitantOrificeAreaVolumetric", TricuspidValveEffectiveRegurgitantOrificeAreaVolumetric),
    ("TricuspidValveMeanGradient", TricuspidValveMeanGradient),
    ("TricuspidValvePeakGradient", TricuspidValvePeakGradient),
    ("TricuspidValvePressureHalfTime", TricuspidValvePressureHalfTime),
    ("TricuspidValveSPrimeVmax", TricuspidValveSPrimeVmax),
    ("TricuspidValveVmax", TricuspidValveVmax),
    ("UAPulsatilityIndex", UAPulsatilityIndex),
    ("UAResistivityIndex", UAResistivityIndex),
    ("USArteryReport", USArteryReport),
    ("USBreastReport", USBreastReport),
    ("USCarotidArteriesReport", USCarotidArteriesReport),
    ("USDopplerAbdominalVesselsReport", USDopplerAbdominalVesselsReport),
    ("USDopplerExtremityVesselsReport", USDopplerExtremityVesselsReport),
    ("USDopplerHeadVesselsReport", USDopplerHeadVesselsReport),
    ("USDopplerLowerExtremityVesselsReport", USDopplerLowerExtremityVesselsReport),
    ("USDopplerUpperExtremityVesselsReport", USDopplerUpperExtremityVesselsReport),
    ("USDopplerVesselsReport", USDopplerVesselsReport),
    ("USKidneyReport", USKidneyReport),
    ("USLiverReport", USLiverReport),
    ("USPancreasReport", USPancreasReport),
    ("USPelvis", USPelvis),
    ("USProstateTransrectalReport", USProstateTransrectalReport),
    ("USScrotumAndTesticleReport", USScrotumAndTesticleReport),
    ("USShoulderReport", USShoulderReport),
    ("USSpleenReport", USSpleenReport),
    ("USTendonReport", USTendonReport),
    ("USThyroidReport", USThyroidReport),
    ("USVeinReport", USVeinReport),
    ("UlnaJeanty1984", UlnaJeanty1984),
    ("UlnaLength", UlnaLength),
    ("UlnaMerz1987", UlnaMerz1987),
    ("UltrasoundObstetricAndGynReport", UltrasoundObstetricAndGynReport),
    ("UltrasoundReport", UltrasoundReport),
    ("UniqueDeviceIdentifier", UniqueDeviceIdentifier),
    ("UterusHeight", UterusHeight),
    ("UterusLength", UterusLength),
    ("UterusVolume", UterusVolume),
    ("UterusWidth", UterusWidth),
    ("VelocityOfFlowPropagation", VelocityOfFlowPropagation),
    ("VelocityRatio", VelocityRatio),
    ("VelocityTimeIntegral", VelocityTimeIntegral),
    ("VenousBloodBaseExcess", VenousBloodBaseExcess),
    ("VenousBloodCarbonDioxideContent", VenousBloodCarbonDioxideContent),
    ("VenousBloodCarbonDioxidePartialPressure", VenousBloodCarbonDioxidePartialPressure),
    ("VenousBloodPH", VenousBloodPH),
    ("VenousOxygenContent", VenousOxygenContent),
    ("VenousOxygenPartialPressure", VenousOxygenPartialPressure),
    ("VenousOxygenPartialPressureAt50PercentSaturation", VenousOxygenPartialPressureAt50PercentSaturation),
    ("VenousOxygenSaturation", VenousOxygenSaturation),
    ("VitalSigns", VitalSigns),
    ("VolumeFlow", VolumeFlow),
    ("WallSegment", WallSegment),
    ("WholeBodyCT", WholeBodyCT),
    ("WidthOfHemisphere", WidthOfHemisphere),
    ("XRUnspecifiedBodyRegion", XRUnspecifiedBodyRegion),
    ("YolkSacLength", YolkSacLength),
];
