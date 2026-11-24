use super::calcium::{ERR_INVALID_OPTIONS, Score, TOTAL, Vessel};
use crate::codes::{dcm, sct, ucum};
use crate::deid::uid::name_uid;
use crate::roi::Error;
use crate::sr::measurement_report::{
    COMPREHENSIVE_3D_SR_STORAGE, MeasurementGroup, MeasurementReport, ReportMeasurement,
    TrackingIdentifier,
};
use crate::sr::{ImageReference, from_code};
pub const ERR_INVALID_MEASUREMENT_REPORT: &str = "dicom/sr: invalid measurement report";
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReportOptions {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub content_date: String,
    pub content_time: String,
    pub image_library: Option<Vec<ImageReference>>,
}
pub fn tracking_identifier(vessel: &Vessel) -> String {
    format!("Coronary calcium: {}", vessel.name())
}
impl Score {
    pub fn measurement_groups(&self, report_uid: &str) -> Result<Vec<MeasurementGroup>, Error> {
        for value in [self.threshold_hu, self.mass_calibration] {
            if !value.is_finite() {
                return Err(Error::wrap(
                    ERR_INVALID_OPTIONS,
                    "non-finite score parameter",
                ));
            }
        }
        let measurement = |concept, value: f64, units| ReportMeasurement {
            concept_name: from_code(concept),
            value,
            units: from_code(units),
            ..ReportMeasurement::default()
        };
        let mut groups = Vec::with_capacity(self.vessels.len() + 1);
        for score in self.vessels.iter().chain(std::iter::once(&self.total)) {
            if score.agatston.is_nan() || score.volume_mm3.is_nan() || score.mass_mg.is_nan() {
                return Err(Error::wrap(
                    ERR_INVALID_OPTIONS,
                    format!("non-finite {} score", score.vessel.as_str()),
                ));
            }
            let mut group = MeasurementGroup {
                tracking: TrackingIdentifier {
                    identifier: tracking_identifier(&score.vessel),
                    uid: name_uid(&[
                        "dcm-rust/roi/calcium tracking",
                        report_uid,
                        score.vessel.as_str(),
                    ]),
                },
                finding: from_code(dcm::Calcium),
                finding_sites: vec![from_code(score.vessel.site())],
                measurements: vec![
                    measurement(
                        sct::CoronaryArteryCalciumScore,
                        score.agatston,
                        ucum::NoUnits,
                    ),
                    measurement(dcm::CalciumVolume, score.volume_mm3, ucum::CubicMillimeter),
                ],
                ..MeasurementGroup::default()
            };
            if self.mass_calibrated {
                group
                    .measurements
                    .push(measurement(dcm::CalciumMass, score.mass_mg, ucum::Mg));
            }
            if score.vessel == TOTAL {
                group.measurements.push(measurement(
                    dcm::AgatstonScoreThreshold,
                    self.threshold_hu,
                    ucum::HounsfieldUnit,
                ));
                if self.mass_calibrated {
                    group.measurements.push(measurement(
                        dcm::CalciumMassThreshold,
                        self.threshold_hu,
                        ucum::HounsfieldUnit,
                    ));
                    group.measurements.push(measurement(
                        dcm::CalciumScoringCalibration,
                        self.mass_calibration,
                        ucum::MgPerHnsfUCm3,
                    ));
                }
            }
            groups.push(group);
        }
        Ok(groups)
    }
    pub fn measurement_report(&self, opts: &ReportOptions) -> Result<MeasurementReport, Error> {
        if opts.sop_instance_uid.is_empty() || opts.study_instance_uid.is_empty() {
            return Err(Error::wrap(
                ERR_INVALID_MEASUREMENT_REPORT,
                "the report needs its SOP Instance and Study Instance UIDs",
            ));
        }
        let groups = self.measurement_groups(&opts.sop_instance_uid)?;
        let sop_class = if opts.sop_class_uid.is_empty() {
            COMPREHENSIVE_3D_SR_STORAGE
        } else {
            &opts.sop_class_uid
        };
        Ok(MeasurementReport {
            sop_class_uid: sop_class.to_string(),
            sop_instance_uid: opts.sop_instance_uid.clone(),
            study_instance_uid: opts.study_instance_uid.clone(),
            series_instance_uid: opts.series_instance_uid.clone(),
            frame_of_reference_uid: opts.frame_of_reference_uid.clone(),
            content_date: opts.content_date.clone(),
            content_time: opts.content_time.clone(),
            procedures_reported: vec![from_code(sct::CardiacCTForCalciumScoring)],
            image_library: Some(opts.image_library.clone().unwrap_or_default()),
            groups,
            ..MeasurementReport::default()
        })
    }
}
