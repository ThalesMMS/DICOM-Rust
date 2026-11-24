use super::{Error, HeaderSpec, Sentinel, WriteOptions, wrap};
use crate::nifti::export::{self, Chunks, Plan, Report};
use crate::nifti::gzip::{Container, DeflateWriter};
use crate::numtext::format_float;
use std::fmt;
#[derive(Debug, Clone, PartialEq)]
pub enum PlanWriteError {
    Meta(Error),
    Export(export::Error),
    Io(String),
}
impl fmt::Display for PlanWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanWriteError::Meta(e) => e.fmt(f),
            PlanWriteError::Export(e) => e.fmt(f),
            PlanWriteError::Io(text) => f.write_str(text),
        }
    }
}
impl std::error::Error for PlanWriteError {}
pub fn write_plan(
    plan: &Plan<'_>,
    options: &WriteOptions,
    temp_dir: &str,
) -> Result<(Vec<u8>, Report), PlanWriteError> {
    let layout = plan.layout();
    if !layout.identity_scaling() {
        return Err(PlanWriteError::Meta(wrap(
            Sentinel::Unsupported,
            format!(
                "value scaling (slope {}, intercept {})",
                format_float(layout.scaling_slope, 'g', -1),
                format_float(layout.scaling_intercept, 'g', -1)
            ),
        )));
    }
    if layout.no_spatial_transform {
        return Err(PlanWriteError::Meta(wrap(
            Sentinel::Unsupported,
            "image grid without patient geometry",
        )));
    }
    let mut spec = HeaderSpec {
        datatype: layout.datatype,
        dimensions: layout.dimensions,
        index_to_lps: layout.index_to_lps,
        fourth_spacing: layout.spacing[3],
        compressed_size: 0,
        options,
    };
    let header = spec.header().map_err(PlanWriteError::Meta)?;
    if !options.compress {
        let mut out = header.into_bytes();
        let mut report = plan
            .write_voxels(&mut out)
            .map_err(PlanWriteError::Export)?;
        report.bytes_written = out.len() as i64;
        return Ok((out, report));
    }
    let spool = crate::nifti::export::spool_probe(temp_dir, "dicom-metaimage-spool-")
        .map_err(|e| PlanWriteError::Io(e.to_string()))?;
    let mut chunks = Chunks::default();
    let report = plan.write_voxels(&mut chunks);
    drop(spool);
    let mut report = report.map_err(PlanWriteError::Export)?;
    let mut compressor = DeflateWriter::new(Container::Zlib);
    for chunk in &chunks.0 {
        compressor.write(chunk);
    }
    let payload = compressor.finish();
    spec.compressed_size = payload.len() as i64;
    let header = spec.header().map_err(PlanWriteError::Meta)?;
    let mut out = header.into_bytes();
    out.extend_from_slice(&payload);
    report.bytes_written = out.len() as i64;
    Ok((out, report))
}
