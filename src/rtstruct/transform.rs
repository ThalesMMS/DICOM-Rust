use super::{Contour, Error, Kind, Point3D, StructureSet};
use crate::render::Vec3;
impl StructureSet {
    pub fn transform<F>(
        &self,
        frame_of_reference_uid: &str,
        mapper: Option<F>,
    ) -> Result<StructureSet, Error>
    where
        F: FnMut(Vec3) -> Result<Vec3, crate::BoxError>,
    {
        let Some(mut mapper) = mapper else {
            return Err(Error::new(
                Kind::Transform,
                "missing structure set or mapper",
            ));
        };
        let mut out = self.clone();
        out.sop_instance_uid.clear();
        out.series_instance_uid.clear();
        out.frame_of_reference_uid = frame_of_reference_uid.to_string();
        for roi in out.rois.iter_mut() {
            let number = roi.number;
            let mut contours = Vec::with_capacity(roi.contours.len());
            for (j, contour) in roi.contours.iter().enumerate() {
                let mut points = Vec::with_capacity(contour.points.len());
                for (k, p) in contour.points.iter().enumerate() {
                    let q = mapper(Vec3::new(p.x, p.y, p.z)).map_err(|e| {
                        Error::new(
                            Kind::Transform,
                            format!("ROI {number} contour {} point {}: {e}", j + 1, k + 1),
                        )
                    })?;
                    if !(q.x + q.y + q.z).is_finite() {
                        return Err(Error::new(
                            Kind::Transform,
                            format!(
                                "ROI {number} contour {} point {} maps to a non-finite point",
                                j + 1,
                                k + 1
                            ),
                        ));
                    }
                    points.push(Point3D::new(q.x, q.y, q.z));
                }
                contours.push(Contour {
                    geometric_type: contour.geometric_type.clone(),
                    points,
                    referenced_images: Vec::new(),
                });
            }
            roi.contours = contours;
        }
        Ok(out)
    }
}
