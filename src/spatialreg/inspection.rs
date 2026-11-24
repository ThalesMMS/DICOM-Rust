use super::{Context, Error, Kind, Registration, undefined_vector};
use crate::render::affine::GeometryAffine;
pub const MAX_DEFORMATION_FIELD_NODES: u64 = 2_097_152;
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeformationFieldDescriptor {
    pub generation: u64,
    pub registered_frame_uid: String,
    pub source_frame_uid: String,
    pub dimensions: [u32; 3],
    pub grid_to_patient_lps: GeometryAffine,
    pub pre: GeometryAffine,
    pub post: GeometryAffine,
}
impl DeformationFieldDescriptor {
    pub fn payload_bytes(&self) -> Result<u64, Error> {
        let mut n = 1u64;
        for size in self.dimensions {
            if !(1..=2048).contains(&size) {
                return Err(Error::plain("deformation grid dimensions outside bounds"));
            }
            n *= u64::from(size);
        }
        if n > MAX_DEFORMATION_FIELD_NODES {
            return Err(Error::plain(format!(
                "deformation grid exceeds {MAX_DEFORMATION_FIELD_NODES} nodes"
            )));
        }
        Ok(n * 16)
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.generation == 0 {
            return Err(Error::plain("deformation field generation required"));
        }
        for id in [&self.registered_frame_uid, &self.source_frame_uid] {
            if crate::dicomjson::trim_unicode_space(id).is_empty() || id.len() > 1024 {
                return Err(Error::plain(
                    "deformation frame identity required and bounded",
                ));
            }
        }
        self.payload_bytes()?;
        for matrix in [&self.grid_to_patient_lps, &self.pre, &self.post] {
            if !matrix.finite() {
                return Err(Error::plain("deformation affine must be finite"));
            }
            let m = &matrix.0;
            if m.iter().any(|v| v.abs() > 1e6) {
                return Err(Error::plain("deformation affine exceeds coordinate bounds"));
            }
            if m[12] != 0.0 || m[13] != 0.0 || m[14] != 0.0 || m[15] != 1.0 {
                return Err(Error::plain("deformation matrix must be affine"));
            }
            let det = m[0] * (m[5] * m[10] - m[6] * m[9]) - m[1] * (m[4] * m[10] - m[6] * m[8])
                + m[2] * (m[4] * m[9] - m[5] * m[8]);
            if det.abs() < 1e-12 || det.abs() > 1e18 {
                return Err(Error::plain(
                    "deformation affine determinant outside bounds",
                ));
            }
        }
        Ok(())
    }
    pub fn validate_payload(&self, data: &[u8]) -> Result<(), Error> {
        self.validate()?;
        let n = self.payload_bytes()?;
        if data.len() as u64 != n {
            return Err(Error::plain("deformation payload length mismatch"));
        }
        let read = |o: usize| f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        for i in (0..data.len()).step_by(16) {
            let valid = read(i + 12);
            if valid != 0.0 && valid != 1.0 {
                return Err(Error::plain("invalid deformation node validity"));
            }
            for j in (0..12).step_by(4) {
                let v = read(i + j);
                let wide = f64::from(v);
                if wide.is_nan()
                    || wide.is_infinite()
                    || wide.abs() > 1e6
                    || (v != 0.0 && wide.abs() < f64::from(f32::MIN_POSITIVE))
                    || (valid == 0.0 && v != 0.0)
                {
                    return Err(Error::plain(format!(
                        "invalid deformation node at {}",
                        i / 16
                    )));
                }
            }
        }
        Ok(())
    }
}
impl Registration {
    pub fn inspection_field(
        &self,
        ctx: &Context,
        generation: u64,
        registered_frame_uid: &str,
    ) -> (DeformationFieldDescriptor, Result<Vec<u8>, Error>) {
        if let Err(e) = ctx.err() {
            return (DeformationFieldDescriptor::default(), Err(e));
        }
        let Some(g) = &self.grid else {
            return (
                DeformationFieldDescriptor::default(),
                Err(Error::new(Kind::InvalidObject, "deformation grid required")),
            );
        };
        let (r, c, n, s, o) = (
            g.row_direction,
            g.column_direction,
            g.normal_direction,
            g.spacing,
            g.origin,
        );
        let m = GeometryAffine([
            r.x * s.x,
            c.x * s.y,
            n.x * s.z,
            o.x,
            r.y * s.x,
            c.y * s.y,
            n.y * s.z,
            o.y,
            r.z * s.x,
            c.z * s.y,
            n.z * s.z,
            o.z,
            0.0,
            0.0,
            0.0,
            1.0,
        ]);
        let d = DeformationFieldDescriptor {
            generation,
            registered_frame_uid: registered_frame_uid.to_string(),
            source_frame_uid: self.source_frame_of_reference_uid.clone(),
            dimensions: g.dimensions,
            grid_to_patient_lps: m,
            pre: self.pre.values,
            post: self.post.values,
        };
        let data = (|| {
            d.validate()?;
            let size = d.payload_bytes()?;
            if g.vectors.len() as u64 * 16 != size {
                return Err(Error::new(Kind::InvalidObject, "grid node count changed"));
            }
            let mut data = vec![0u8; size as usize];
            for (i, v) in g.vectors.iter().enumerate() {
                if i % 4096 == 0 {
                    ctx.err()?;
                }
                if undefined_vector(*v) {
                    continue;
                }
                for (a, value) in [v[0], v[1], v[2], 1.0].into_iter().enumerate() {
                    data[i * 16 + a * 4..i * 16 + a * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
            }
            d.validate_payload(&data)?;
            ctx.err()?;
            Ok(data)
        })();
        (d, data)
    }
}
