use super::segment::{self as seg, CodedEntry, ReferencedImage};
use super::{
    Algorithm, Document, Error, Kind, Primitives, Segment, Surface, SurfaceReference, invalid,
};
use crate::dicomjson::trim_unicode_space;
use crate::mesh::{self, Mesh};
#[derive(Clone, Debug, Default)]
pub struct MeshSegment {
    pub segment: seg::Segment,
    pub color_rgb: [u8; 3],
    pub mesh: Option<Mesh>,
    pub opacity: f64,
}
#[derive(Clone, Debug, Default)]
pub struct FromMeshesOptions {
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub segments: Vec<MeshSegment>,
    pub algorithm: Algorithm,
    pub source_images: Vec<ReferencedImage>,
}
pub fn surface_nets_algorithm(version: &str) -> Algorithm {
    Algorithm {
        family: CodedEntry::new("123101", "DCM", "Neighborhood Analysis"),
        name: "SurfaceNets".to_string(),
        version: version.to_string(),
        ..Algorithm::default()
    }
}
pub fn from_meshes(opts: &FromMeshesOptions) -> Result<Document, Error> {
    let mut doc = Document {
        sop_instance_uid: opts.sop_instance_uid.clone(),
        study_instance_uid: opts.study_instance_uid.clone(),
        series_instance_uid: opts.series_instance_uid.clone(),
        frame_of_reference_uid: opts.frame_of_reference_uid.clone(),
        referenced_images: opts.source_images.clone(),
        ..Document::default()
    };
    for (i, part) in opts.segments.iter().enumerate() {
        let Some(part_mesh) = &part.mesh else {
            return Err(invalid(format!("segment {} has no mesh", i + 1)));
        };
        let mut segment = part.segment.clone();
        if segment.number == 0 {
            segment.number = i as i64 + 1;
        }
        if trim_unicode_space(&segment.label).is_empty() {
            segment.label = format!("Segment {}", segment.number);
        }
        if segment.algorithm_type.is_empty() {
            segment.algorithm_type = seg::ALGORITHM_MANUAL.to_string();
        }
        if segment.segmented_property_category == CodedEntry::default() {
            segment.segmented_property_category = seg::generic_roi_category_code();
        }
        if segment.segmented_property_type == CodedEntry::default() {
            segment.segmented_property_type = seg::generic_roi_type_code();
        }
        if segment.recommended_display_cielab == [0; 3] {
            segment.recommended_display_cielab =
                seg::rgb_to_dicom_cielab(part.color_rgb.map(i64::from));
        }
        let sources = opts
            .source_images
            .iter()
            .map(|source| ReferencedImage {
                series_instance_uid: String::new(),
                sop_class_uid: source.sop_class_uid.clone(),
                sop_instance_uid: source.sop_instance_uid.clone(),
                frames: source.frames.clone(),
            })
            .collect();
        let number = doc.surfaces.len() as i64 + 1;
        doc.surfaces.push(Surface {
            number,
            mesh: Some(part_mesh.clone()),
            recommended_display_cielab: segment.recommended_display_cielab,
            opacity: part.opacity,
            ..Surface::default()
        });
        doc.segments.push(Segment {
            segment,
            surfaces: vec![SurfaceReference {
                number,
                algorithm: opts.algorithm.clone(),
                source_instances: sources,
            }],
        });
    }
    doc.validate()?;
    Ok(doc)
}
fn lab_to_rgb(lab: [u16; 3]) -> [u8; 3] {
    seg::dicom_cielab_to_rgb(lab).map(|v| v.clamp(0, 255) as u8)
}
impl Document {
    pub fn segment_mesh(&self, number: i64) -> Option<(Mesh, [u8; 3])> {
        let segment = self.segments.iter().find(|s| s.segment.number == number)?;
        let mut meshes: Vec<&Mesh> = Vec::new();
        let mut lab = segment.segment.recommended_display_cielab;
        for reference in &segment.surfaces {
            if let Some((surface, m)) = self
                .surface(reference.number)
                .and_then(|s| s.mesh.as_ref().map(|m| (s, m)))
            {
                meshes.push(m);
                if lab == [0; 3] {
                    lab = surface.recommended_display_cielab;
                }
            }
        }
        let merged = match meshes.as_slice() {
            [] => return None,
            [one] => (*one).clone(),
            many => {
                let optional: Vec<Option<&Mesh>> = many.iter().map(|m| Some(*m)).collect();
                mesh::merge(&optional, None).0
            }
        };
        Some((merged, lab_to_rgb(lab)))
    }
    pub fn segment_primitives(&self, number: i64) -> Primitives {
        let mut result = Primitives::default();
        let Some(segment) = self.segments.iter().find(|s| s.segment.number == number) else {
            return result;
        };
        let mut offset = 0u32;
        for reference in &segment.surfaces {
            let Some((s, m)) = self
                .surface(reference.number)
                .and_then(|s| s.mesh.as_ref().map(|m| (s, m)))
            else {
                continue;
            };
            let shift = |v: &u32| v.wrapping_add(offset);
            result
                .vertices
                .extend(s.primitives.vertices.iter().map(shift));
            result.edges.extend(
                s.primitives
                    .edges
                    .iter()
                    .map(|e| [shift(&e[0]), shift(&e[1])]),
            );
            result.lines.extend(
                s.primitives
                    .lines
                    .iter()
                    .map(|l| l.iter().map(shift).collect()),
            );
            offset = offset.wrapping_add(m.vertices.len() as u32);
        }
        result
    }
    pub fn parts(&self) -> Vec<(String, Mesh, [u8; 3])> {
        self.segments
            .iter()
            .filter_map(|segment| {
                self.segment_mesh(segment.segment.number)
                    .map(|(m, color)| (segment.segment.label.clone(), m, color))
            })
            .collect()
    }
    pub fn triangle_parts(&self) -> Result<Vec<(String, Mesh, [u8; 3])>, Error> {
        for s in &self.surfaces {
            if s.primitives.count() != 0 {
                return Err(Error::new(
                    Kind::NonTrianglePrimitives,
                    format!("surface {}", s.number),
                ));
            }
        }
        Ok(self.parts())
    }
}
