use super::{Error, Result, hex};
use crate::pixel::Metadata;
use crate::render::frame::Frame;
use crate::render::volume::{Stack, Volume, build_volume};
use crate::render::volume_store::{
    VOLUME_SNAPSHOT_CONTRACT_VERSION, VOLUME_SNAPSHOT_HEADER_SIZE_V1, VolumeDerivation,
    VolumeDescriptor, VolumeLease, VolumeSampleDomain, VolumeScalarFormat, VolumeStore,
    validate_descriptor,
};
use crate::render::{GeometryAffine, Rescale, WindowLevel};
use crate::textfmt::quote;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::sync::Arc;
pub const SYNTHETIC_CT_NAME: &str = "synth-ct-i16le-v1";
pub const REFERENCE_SYNTHETIC_CT_SHA256: &str =
    "1531f7807ee24b01a802eb6b0f75657d4faae6beb28ff7dd1b98682da5eca46b";
const REFERENCE_DIMENSIONS: [u32; 3] = [512, 512, 300];
pub fn reference_synthetic_ct_dimensions() -> [u32; 3] {
    REFERENCE_DIMENSIONS
}
#[derive(Debug, Clone)]
pub struct MaterializedVolume {
    name: String,
    descriptor: VolumeDescriptor,
    payload: Arc<[u8]>,
    sha256: [u8; 32],
}
fn materialize(
    name: &str,
    descriptor: VolumeDescriptor,
    value: impl Fn(u32, u32, u32) -> i16,
) -> Result<MaterializedVolume> {
    let [w, h, d] = descriptor.dimensions;
    let voxels = u64::from(w) * u64::from(h) * u64::from(d);
    let bytes = usize::try_from(voxels * 2)
        .map_err(|_| Error::other("qualification: payload exceeds addressable memory"))?;
    let mut payload: Vec<u8> = Vec::new();
    payload
        .try_reserve_exact(bytes)
        .map_err(|_| Error::other("qualification: payload allocation failed"))?;
    for z in 0..d {
        for y in 0..h {
            for x in 0..w {
                payload.extend_from_slice(&value(x, y, z).to_le_bytes());
            }
        }
    }
    let sha256: [u8; 32] = Sha256::digest(&payload).into();
    Ok(MaterializedVolume {
        name: name.to_string(),
        descriptor,
        payload: payload.into(),
        sha256,
    })
}
pub fn new_synthetic_ct(dimensions: [u32; 3], generation: u64) -> Result<MaterializedVolume> {
    if generation == 0 {
        return Err(Error::other("qualification: zero volume generation"));
    }
    for (axis, dimension) in dimensions.iter().enumerate() {
        if *dimension == 0 {
            return Err(Error::other(format!(
                "qualification: zero dimension at axis {axis}"
            )));
        }
    }
    let mut voxels = u64::from(dimensions[0]);
    for dimension in &dimensions[1..] {
        voxels = voxels
            .checked_mul(u64::from(*dimension))
            .ok_or_else(|| Error::other("qualification: voxel count overflow"))?;
    }
    if voxels > (i64::MAX as u64) / 2 {
        return Err(Error::other(
            "qualification: payload exceeds the slice capacity",
        ));
    }
    let descriptor = synthetic_ct_descriptor(dimensions, generation)?;
    let result = materialize(SYNTHETIC_CT_NAME, descriptor, |x, y, z| {
        (((u64::from(x) * 3 + u64::from(y) * 5 + u64::from(z) * 7) % 4096) as i64 - 1024) as i16
    })?;
    if dimensions == REFERENCE_DIMENSIONS
        && result.payload_sha256() != REFERENCE_SYNTHETIC_CT_SHA256
    {
        return Err(Error::other(format!(
            "qualification: reference payload SHA-256 {}, want {REFERENCE_SYNTHETIC_CT_SHA256}",
            result.payload_sha256()
        )));
    }
    Ok(result)
}
fn stored_i16_descriptor(
    dimensions: [u32; 3],
    spacing: [f64; 3],
    generation: u64,
) -> VolumeDescriptor {
    let row_bytes = u64::from(dimensions[0]).wrapping_mul(2);
    let slice_bytes = u64::from(dimensions[1]).wrapping_mul(row_bytes);
    let byte_length = u64::from(dimensions[2]).wrapping_mul(slice_bytes);
    let [sx, sy, sz] = spacing;
    VolumeDescriptor {
        contract_version: VOLUME_SNAPSHOT_CONTRACT_VERSION,
        header_size: VOLUME_SNAPSHOT_HEADER_SIZE_V1,
        volume_generation: generation,
        parent_generation: 0,
        derivation: VolumeDerivation::NORMALIZED,
        dimensions,
        components: 1,
        scalar_format: VolumeScalarFormat::I16_STORED_LE,
        sample_domain: VolumeSampleDomain::STORED,
        row_stride_bytes: row_bytes,
        slice_stride_bytes: slice_bytes,
        byte_length,
        rescale_slope: 1.0,
        rescale_intercept: 0.0,
        spacing_mm: spacing,
        index_to_patient_lps: GeometryAffine([
            sx, 0.0, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, 0.0, sz, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]),
        patient_lps_to_index: GeometryAffine([
            1.0 / sx,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0 / sy,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0 / sz,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ]),
    }
}
pub(crate) fn synthetic_ct_descriptor(
    dimensions: [u32; 3],
    generation: u64,
) -> Result<VolumeDescriptor> {
    let descriptor = stored_i16_descriptor(dimensions, [0.7, 0.7, 1.0], generation);
    validate_descriptor(&descriptor)
        .map_err(|e| Error::from(e).prefixed("qualification: synthetic CT descriptor"))?;
    Ok(descriptor)
}
pub fn new_reference_synthetic_ct(generation: u64) -> Result<MaterializedVolume> {
    new_synthetic_ct(REFERENCE_DIMENSIONS, generation)
}
impl MaterializedVolume {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn descriptor(&self) -> VolumeDescriptor {
        self.descriptor
    }
    pub fn payload_sha256(&self) -> String {
        hex(&self.sha256)
    }
    pub fn payload_size(&self) -> u64 {
        self.payload.len() as u64
    }
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
    pub fn copy_payload(&self) -> Vec<u8> {
        self.payload.to_vec()
    }
    pub fn write_payload(&self, dst: &mut dyn Write) -> Result<()> {
        dst.write_all(&self.payload).map_err(|e| Error::io(&e))
    }
    pub fn load(&self, store: &Arc<VolumeStore>) -> Result<VolumeLease> {
        let generation = store.replace(self.descriptor, &self.payload)?;
        Ok(store.acquire(generation)?)
    }
    pub fn verify(&self) -> Result<()> {
        if self.name != SYNTHETIC_CT_NAME && clinical_fixture_definition_by_id(&self.name).is_none()
        {
            return Err(Error::other(format!(
                "qualification: unknown fixture name {}",
                quote(&self.name)
            )));
        }
        validate_descriptor(&self.descriptor)?;
        if self.payload.len() as u64 != self.descriptor.byte_length {
            return Err(Error::other(format!(
                "qualification: payload length {}, descriptor byte length {}",
                self.payload.len(),
                self.descriptor.byte_length
            )));
        }
        let digest: [u8; 32] = Sha256::digest(&self.payload).into();
        if digest != self.sha256 {
            return Err(Error::other("qualification: payload SHA-256 drift"));
        }
        if let Some(expected) = reference_clinical_fixture_sha256(&self.name)
            && hex(&digest) != expected
        {
            return Err(Error::other("qualification: frozen fixture SHA-256 drift"));
        }
        Ok(())
    }
    pub fn render_stack(&self) -> Result<Stack> {
        self.verify()?;
        let descriptor = self.descriptor;
        if descriptor.scalar_format != VolumeScalarFormat::I16_STORED_LE
            || descriptor.sample_domain != VolumeSampleDomain::STORED
            || descriptor.components != 1
        {
            return Err(Error::other(
                "qualification: generated render stack requires stored mono int16",
            ));
        }
        if descriptor.dimensions[0] > u32::from(u16::MAX)
            || descriptor.dimensions[1] > u32::from(u16::MAX)
        {
            return Err(Error::other(
                "qualification: fixture dimensions exceed DICOM frame fields",
            ));
        }
        let window = WindowLevel {
            center: 200.0,
            width: 244.0,
            ..Default::default()
        };
        let spacing = descriptor.spacing_mm;
        let mut frames = Vec::with_capacity(descriptor.dimensions[2] as usize);
        for z in 0..descriptor.dimensions[2] {
            let start = u64::from(z) * descriptor.slice_stride_bytes;
            let end = start + descriptor.slice_stride_bytes;
            if end > self.payload.len() as u64 {
                return Err(Error::other(format!(
                    "qualification: fixture slice {z} exceeds payload"
                )));
            }
            let a = &descriptor.index_to_patient_lps.0;
            let zf = f64::from(z);
            let origin = [
                a[0] * 0.0 + a[1] * 0.0 + a[2] * zf + a[3],
                a[4] * 0.0 + a[5] * 0.0 + a[6] * zf + a[7],
                a[8] * 0.0 + a[9] * 0.0 + a[10] * zf + a[11],
            ];
            frames.push(Some(Arc::new(Frame {
                sop_instance_uid: format!("1.2.826.0.1.3680043.10.5432.886.{}", z + 1),
                frame_index: i64::from(z),
                metadata: Metadata {
                    rows: descriptor.dimensions[1] as u16,
                    columns: descriptor.dimensions[0] as u16,
                    samples_per_pixel: 1,
                    bits_allocated: 16,
                    bits_stored: 16,
                    high_bit: 15,
                    pixel_representation: 1,
                    number_of_frames: 1,
                    photometric_interpretation: "MONOCHROME2".to_string(),
                    ..Default::default()
                },
                big_endian: false,
                pixel_bytes: Arc::new(self.payload[start as usize..end as usize].to_vec()),
                default_window: window.clone(),
                rescale: Rescale {
                    slope: descriptor.rescale_slope,
                    intercept: descriptor.rescale_intercept,
                },
                image_position: origin.to_vec(),
                image_orientation: vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                pixel_spacing: vec![spacing[1], spacing[0]],
                slice_thickness: spacing[2],
                slice_location: origin[2],
                slice_location_ok: true,
                instance_number: i64::from(z) + 1,
                sort: origin[2],
                ..Default::default()
            })));
        }
        let mut stack = Stack::new(frames);
        stack.uid = "1.2.826.0.1.3680043.10.5432.886".to_string();
        stack.study_uid = "1.2.826.0.1.3680043.10.5432.878".to_string();
        stack.modality = "CT".to_string();
        stack.body_part = "SYNTHETIC".to_string();
        stack.default_window = window;
        stack.pixel_spacing = vec![spacing[1], spacing[0]];
        stack.slice_thickness = spacing[2];
        Ok(stack)
    }
    pub fn build_render_volume(&self) -> Result<Arc<Volume>> {
        let stack = self.render_stack()?;
        Ok(build_volume(&stack)?)
    }
}
pub const CLINICAL_FIXTURE_RAMP_HU: &str = "clinical-ramp-hu-v1";
pub const CLINICAL_FIXTURE_STEPS: &str = "clinical-steps-v1";
pub const CLINICAL_FIXTURE_CONCENTRIC_SPHERES: &str = "clinical-concentric-spheres-v1";
pub const CLINICAL_FIXTURE_CONTRAST_TUBES: &str = "clinical-contrast-tubes-v1";
pub const CLINICAL_FIXTURE_AIR_STRUCTURE: &str = "clinical-air-structure-v1";
pub const CLINICAL_FIXTURE_SLICE_IMPULSE: &str = "clinical-slice-impulse-v1";
pub const CLINICAL_FIXTURE_UNIFORM: &str = "clinical-uniform-v1";
pub const CLINICAL_FIXTURE_ANISOTROPIC: &str = "clinical-anisotropic-v1";
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClinicalFixtureDefinition {
    pub id: &'static str,
    pub description: &'static str,
    pub dimensions: [u32; 3],
    pub spacing_mm: [f64; 3],
}
const fn definition(
    id: &'static str,
    description: &'static str,
    dimensions: [u32; 3],
    spacing_mm: [f64; 3],
) -> ClinicalFixtureDefinition {
    ClinicalFixtureDefinition {
        id,
        description,
        dimensions,
        spacing_mm,
    }
}
const UNIT: [f64; 3] = [1.0, 1.0, 1.0];
static CLINICAL_FIXTURE_CATALOG_V1: [ClinicalFixtureDefinition; 8] = [
    definition(
        CLINICAL_FIXTURE_RAMP_HU,
        "HU ramp below, within, and above every clinical window",
        [32, 8, 8],
        UNIT,
    ),
    definition(
        CLINICAL_FIXTURE_STEPS,
        "constant sentinel-intensity regions",
        [32, 8, 8],
        UNIT,
    ),
    definition(
        CLINICAL_FIXTURE_CONCENTRIC_SPHERES,
        "nested densities for front-to-back accumulation and occlusion",
        [32, 32, 32],
        UNIT,
    ),
    definition(
        CLINICAL_FIXTURE_CONTRAST_TUBES,
        "high-contrast vascular tubes",
        [32, 32, 24],
        UNIT,
    ),
    definition(
        CLINICAL_FIXTURE_AIR_STRUCTURE,
        "soft-tissue shell with an air cavity",
        [32, 32, 32],
        UNIT,
    ),
    definition(
        CLINICAL_FIXTURE_SLICE_IMPULSE,
        "one distinct impulse per source slice",
        [9, 9, 7],
        UNIT,
    ),
    definition(
        CLINICAL_FIXTURE_UNIFORM,
        "degenerate constant-value domain",
        [16, 16, 16],
        UNIT,
    ),
    definition(
        CLINICAL_FIXTURE_ANISOTROPIC,
        "physical-space structures with anisotropic spacing",
        [32, 24, 12],
        [0.5, 0.8, 3.0],
    ),
];
const CLINICAL_FIXTURE_SHA256_V1: [&str; 8] = [
    "bbe28543c7d99d88d0033efef6fe2bb5657827e84808d2537bc4f87ad0d256c8",
    "8ed3f63f25001cc8c1c29bc2cf17c72739805e128ec4ed8f4c7bc3f4c802eeb9",
    "e7e6e223e491643d65e5381d9b8c66793b0881a6639433c5ba6f4978d81f683f",
    "3ec622a27ec5a00abfd65f313744812b6aadb9f2cb41cc12c671423bbf2397c2",
    "37a8d6616b23cd1e995abf9ae7c3e103aa473535a3e300a67f044d2ebc37a402",
    "b5223fcf924dfec259566884d7ba94a768d4d7fa2a3b313581d96a2b0aaa5ccc",
    "23c9c8514cd4f02ba3ef519dc461c3d56ce4639cd3b0a8ad1d5d2bb417ca0714",
    "a70a1de7ea8af85dcb7da2c33d1c5acbf41f43b26e5396fb50245084b12c97ba",
];
pub fn clinical_fixture_catalog() -> &'static [ClinicalFixtureDefinition] {
    &CLINICAL_FIXTURE_CATALOG_V1
}
pub fn clinical_fixture_definition_by_id(id: &str) -> Option<ClinicalFixtureDefinition> {
    CLINICAL_FIXTURE_CATALOG_V1
        .iter()
        .find(|d| d.id == id)
        .copied()
}
pub fn reference_clinical_fixture_sha256(id: &str) -> Option<&'static str> {
    CLINICAL_FIXTURE_CATALOG_V1
        .iter()
        .position(|d| d.id == id)
        .map(|i| CLINICAL_FIXTURE_SHA256_V1[i])
}
pub fn new_clinical_fixture(id: &str, generation: u64) -> Result<MaterializedVolume> {
    let Some(definition) = clinical_fixture_definition_by_id(id) else {
        return Err(Error::other(format!(
            "qualification: unknown clinical fixture {}",
            quote(id)
        )));
    };
    if generation == 0 {
        return Err(Error::other("qualification: zero volume generation"));
    }
    let descriptor =
        stored_i16_descriptor(definition.dimensions, definition.spacing_mm, generation);
    validate_descriptor(&descriptor)
        .map_err(|e| Error::from(e).prefixed("qualification: clinical fixture descriptor"))?;
    let result = materialize(definition.id, descriptor, |x, y, z| {
        clinical_fixture_value(&definition, x, y, z)
    })?;
    if let Some(expected) = reference_clinical_fixture_sha256(id)
        && result.payload_sha256() != expected
    {
        return Err(Error::other(format!(
            "qualification: clinical fixture {} SHA-256 {}, want {expected}",
            quote(id),
            result.payload_sha256()
        )));
    }
    Ok(result)
}
fn clinical_fixture_value(definition: &ClinicalFixtureDefinition, x: u32, y: u32, z: u32) -> i16 {
    let [w, h, d] = definition.dimensions;
    let (xf, yf, zf) = (f64::from(x), f64::from(y), f64::from(z));
    match definition.id {
        CLINICAL_FIXTURE_RAMP_HU => {
            if w <= 1 {
                return -1400;
            }
            (-1400 + i64::from(x) * 3200 / i64::from(w - 1)) as i16
        }
        CLINICAL_FIXTURE_STEPS => {
            const SENTINELS: [i16; 11] =
                [-1200, -1000, -800, -550, -300, 0, 200, 350, 700, 1200, 2500];
            let index = (u64::from(x) * SENTINELS.len() as u64 / u64::from(w)) as usize;
            SENTINELS[index.min(SENTINELS.len() - 1)]
        }
        CLINICAL_FIXTURE_CONCENTRIC_SPHERES => {
            let distance = normalized_radius_squared(x, y, z, definition);
            if distance <= 0.01 {
                1350
            } else if distance <= 0.0484 {
                420
            } else if distance <= 0.1444 {
                60
            } else {
                -1000
            }
        }
        CLINICAL_FIXTURE_CONTRAST_TUBES => {
            let (cx, cy) = (f64::from(w - 1) / 2.0, f64::from(h - 1) / 2.0);
            let (dx1, dy1) = (xf - cx * 0.55, yf - cy);
            let (dx2, dy2) = (xf - cx * 1.45, yf - cy);
            if dx1 * dx1 + dy1 * dy1 <= 5.76 {
                return 1100;
            }
            if dx2 * dx2 + dy2 * dy2 <= 10.24 {
                return 520;
            }
            if (yf - cy).abs() <= 1.25 && (zf - f64::from(d - 1) / 2.0).abs() <= 1.25 {
                return 760;
            }
            -850
        }
        CLINICAL_FIXTURE_AIR_STRUCTURE => {
            let distance = normalized_radius_squared(x, y, z, definition);
            if distance > 0.16 {
                return -1000;
            }
            let (wf, hf, df) = (f64::from(w), f64::from(h), f64::from(d));
            let air_dx = (xf - wf * 0.57) / wf;
            let air_dy = (yf - hf * 0.48) / hf;
            let air_dz = (zf - df * 0.50) / df;
            let air_distance = air_dx * air_dx + air_dy * air_dy + air_dz * air_dz;
            if air_distance <= 0.0256 {
                -980
            } else if air_distance <= 0.04 {
                720
            } else {
                65
            }
        }
        CLINICAL_FIXTURE_SLICE_IMPULSE => {
            let impulse_x = (1 + (i64::from(z) * 3) % i64::from(w - 2)) as u32;
            let impulse_y = (1 + (i64::from(z) * 5) % i64::from(h - 2)) as u32;
            if x == impulse_x && y == impulse_y {
                return (500 + i64::from(z) * 350) as i16;
            }
            0
        }
        CLINICAL_FIXTURE_UNIFORM => 42,
        CLINICAL_FIXTURE_ANISOTROPIC => {
            let distance = normalized_radius_squared(x, y, z, definition);
            if distance <= 0.0256 {
                920
            } else if distance <= 0.1296 {
                240
            } else {
                -1000
            }
        }
        _ => 0,
    }
}
fn normalized_radius_squared(
    x: u32,
    y: u32,
    z: u32,
    definition: &ClinicalFixtureDefinition,
) -> f64 {
    let [w, h, d] = definition.dimensions;
    let spacing = definition.spacing_mm;
    let extents = [
        f64::from(w - 1) * spacing[0],
        f64::from(h - 1) * spacing[1],
        f64::from(d - 1) * spacing[2],
    ];
    let maximum = extents[0].max(extents[1].max(extents[2]));
    if maximum <= 0.0 {
        return 0.0;
    }
    let dx = (f64::from(x) * spacing[0] - extents[0] / 2.0) / maximum;
    let dy = (f64::from(y) * spacing[1] - extents[1] / 2.0) / maximum;
    let dz = (f64::from(z) * spacing[2] - extents[2] / 2.0) / maximum;
    dx * dx + dy * dy + dz * dz
}
