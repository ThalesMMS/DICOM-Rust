use super::{Error, Result, blank, hex, trim_unicode_space};
use crate::render::volume_store::{
    VolumeDescriptor, VolumeLease, VolumeSampleDomain, VolumeScalarFormat, VolumeSnapshot,
    validate_descriptor,
};
use crate::textfmt::quote;
use sha2::{Digest, Sha256};
pub const TRANSPORT_CPU_CANONICAL_F32: &str = "cpu-canonical-f32le";
pub const TRANSPORT_VTK_I16: &str = "vtk-i16le";
pub const TRANSPORT_WEBGPU_R32_FLOAT: &str = "webgpu-r32float-transport";
#[derive(Debug, Clone, PartialEq)]
pub struct VolumeTransport {
    kind: &'static str,
    descriptor: VolumeDescriptor,
    payload: Vec<u8>,
    sha256: [u8; 32],
}
impl VolumeTransport {
    pub fn kind(&self) -> &'static str {
        self.kind
    }
    pub fn descriptor(&self) -> VolumeDescriptor {
        self.descriptor
    }
    pub fn generation(&self) -> u64 {
        self.descriptor.volume_generation
    }
    pub fn payload_sha256(&self) -> String {
        hex(&self.sha256)
    }
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
    pub fn copy_payload(&self) -> Vec<u8> {
        self.payload.clone()
    }
    pub fn verify(&self) -> Result<()> {
        if ![
            TRANSPORT_CPU_CANONICAL_F32,
            TRANSPORT_VTK_I16,
            TRANSPORT_WEBGPU_R32_FLOAT,
        ]
        .contains(&self.kind)
        {
            return Err(Error::other(format!(
                "qualification: unknown transport {}",
                quote(self.kind)
            )));
        }
        validate_descriptor(&self.descriptor)?;
        if self.payload.len() as u64 != self.descriptor.byte_length {
            return Err(Error::other(format!(
                "qualification: transport payload length {}, descriptor byte length {}",
                self.payload.len(),
                self.descriptor.byte_length
            )));
        }
        let digest: [u8; 32] = Sha256::digest(&self.payload).into();
        if digest != self.sha256 {
            return Err(Error::other("qualification: transport SHA-256 drift"));
        }
        Ok(())
    }
}
pub fn adapt_cpu_canonical_f32(lease: &VolumeLease) -> Result<VolumeTransport> {
    adapt_modality_f32(lease, TRANSPORT_CPU_CANONICAL_F32)
}
pub fn adapt_vtk_i16(lease: &VolumeLease) -> Result<VolumeTransport> {
    let (snapshot, descriptor) = authoritative_snapshot(lease)?;
    if descriptor.scalar_format != VolumeScalarFormat::I16_STORED_LE
        || descriptor.sample_domain != VolumeSampleDomain::STORED
    {
        return Err(Error::other(format!(
            "qualification: VTK I16 transport requires I16_STORED_LE, got {}/{}",
            descriptor.scalar_format.name(),
            descriptor.sample_domain.name()
        )));
    }
    let Ok(capacity) = usize::try_from(descriptor.byte_length) else {
        return Err(Error::other(
            "qualification: VTK transport exceeds the buffer capacity",
        ));
    };
    let mut output = Vec::with_capacity(capacity);
    snapshot.write_payload_to(&mut output)?;
    new_transport(TRANSPORT_VTK_I16, descriptor, output)
}
pub fn adapt_webgpu_r32_float_transport(lease: &VolumeLease) -> Result<VolumeTransport> {
    adapt_modality_f32(lease, TRANSPORT_WEBGPU_R32_FLOAT)
}
fn adapt_modality_f32(lease: &VolumeLease, kind: &'static str) -> Result<VolumeTransport> {
    let (snapshot, mut descriptor) = authoritative_snapshot(lease)?;
    let voxels = u64::from(descriptor.dimensions[0])
        .wrapping_mul(u64::from(descriptor.dimensions[1]))
        .wrapping_mul(u64::from(descriptor.dimensions[2]));
    if voxels > (i64::MAX as u64) / 4 {
        return Err(Error::other(
            "qualification: F32 transport exceeds the buffer capacity",
        ));
    }
    let mut output = Vec::with_capacity(usize::try_from(voxels * 4).unwrap_or(0));
    snapshot.write_modality_f32_to(&mut output)?;
    descriptor.scalar_format = VolumeScalarFormat::F32_MODALITY_LE;
    descriptor.sample_domain = VolumeSampleDomain::MODALITY;
    descriptor.row_stride_bytes = u64::from(descriptor.dimensions[0]) * 4;
    descriptor.slice_stride_bytes =
        u64::from(descriptor.dimensions[1]).wrapping_mul(descriptor.row_stride_bytes);
    descriptor.byte_length = voxels * 4;
    descriptor.rescale_slope = 1.0;
    descriptor.rescale_intercept = 0.0;
    new_transport(kind, descriptor, output)
}
pub(crate) fn authoritative_snapshot(
    lease: &VolumeLease,
) -> Result<(VolumeSnapshot, VolumeDescriptor)> {
    let snapshot = lease.snapshot()?;
    let descriptor = snapshot.descriptor()?;
    let generation = snapshot.generation();
    if descriptor.volume_generation == 0 || descriptor.volume_generation != generation {
        return Err(Error::other(format!(
            "qualification: authoritative generation mismatch descriptor={} snapshot={generation}",
            descriptor.volume_generation
        )));
    }
    Ok((snapshot, descriptor))
}
fn new_transport(
    kind: &'static str,
    descriptor: VolumeDescriptor,
    payload: Vec<u8>,
) -> Result<VolumeTransport> {
    let sha256: [u8; 32] = Sha256::digest(&payload).into();
    let result = VolumeTransport {
        kind,
        descriptor,
        payload,
        sha256,
    };
    result.verify()?;
    Ok(result)
}
pub const CAPABILITY_AVAILABLE: &str = "available";
pub const CAPABILITY_UNAVAILABLE: &str = "unavailable";
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BackendCapability {
    pub backend: String,
    pub feature: String,
    pub status: String,
    pub reason: String,
}
impl BackendCapability {
    pub fn validate(&self) -> Result<()> {
        if blank(&self.backend) || blank(&self.feature) {
            return Err(Error::other(
                "qualification: capability lacks backend or feature",
            ));
        }
        match self.status.as_str() {
            CAPABILITY_AVAILABLE => {
                if !self.reason.is_empty() {
                    return Err(Error::other(
                        "qualification: available capability has unavailable reason",
                    ));
                }
            }
            CAPABILITY_UNAVAILABLE => {
                if blank(&self.reason) {
                    return Err(Error::other(
                        "qualification: unavailable capability lacks reason",
                    ));
                }
            }
            other => {
                return Err(Error::other(format!(
                    "qualification: invalid capability status {}",
                    quote(other)
                )));
            }
        }
        Ok(())
    }
}
pub fn webgpu_volume_renderer_capability(
    worker_available: bool,
    unavailable_reason: &str,
) -> BackendCapability {
    let mut capability = BackendCapability {
        backend: "webgpu-wgpu-native-worker".to_string(),
        feature: "volume-renderer".to_string(),
        ..Default::default()
    };
    if worker_available {
        capability.status = CAPABILITY_AVAILABLE.to_string();
        return capability;
    }
    capability.status = CAPABILITY_UNAVAILABLE.to_string();
    capability.reason = trim_unicode_space(unavailable_reason).to_string();
    if capability.reason.is_empty() {
        capability.reason = "worker start/handshake probe failed".to_string();
    }
    capability
}
