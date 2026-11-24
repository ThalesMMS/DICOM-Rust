use super::{Error, Sentinel, escape_key_value, truncate, wrap};
use crate::nifti::gzip::{Container, DeflateWriter};
use crate::nifti::{self, Volume};
use crate::numtext;
use crate::object::error::quote_bytes;
use crate::render::GeometryAffine;
use std::collections::BTreeMap;
fn type_by_datatype(datatype: i16) -> Option<(&'static str, usize)> {
    Some(match datatype {
        nifti::DATATYPE_INT8 => ("int8", 1),
        nifti::DATATYPE_UINT8 => ("uint8", 1),
        nifti::DATATYPE_INT16 => ("int16", 2),
        nifti::DATATYPE_UINT16 => ("uint16", 2),
        nifti::DATATYPE_INT32 => ("int32", 4),
        nifti::DATATYPE_UINT32 => ("uint32", 4),
        nifti::DATATYPE_FLOAT32 => ("float", 4),
        nifti::DATATYPE_FLOAT64 => ("double", 8),
        _ => return None,
    })
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WriteOptions {
    pub raw: bool,
    pub key_values: BTreeMap<String, String>,
    pub fourth_axis_kind: String,
}
pub(crate) struct HeaderSpec<'a> {
    pub datatype: i16,
    pub dimensions: [i64; 4],
    pub index_to_lps: GeometryAffine,
    pub options: &'a WriteOptions,
    pub list_first: bool,
}
fn format_number(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    numtext::format_float(v, 'g', 17)
}
impl HeaderSpec<'_> {
    pub(crate) fn header(&self) -> Result<String, Error> {
        let Some((typ, width)) = type_by_datatype(self.datatype) else {
            return Err(wrap(
                Sentinel::Unsupported,
                format!("datatype {}", self.datatype),
            ));
        };
        for (key, value) in &self.options.key_values {
            if key.is_empty()
                || key.contains(":=")
                || key.contains(['\r', '\n'])
                || value.contains('\r')
            {
                return Err(wrap(
                    Sentinel::MalformedHeader,
                    format!("key/value {}", quote_bytes(&truncate(key.as_bytes()))),
                ));
            }
        }
        let d = &self.dimensions;
        for axis in 0..3 {
            if d[axis] < 1 {
                return Err(wrap(
                    Sentinel::MalformedHeader,
                    format!("size {} of axis {}", d[axis], axis),
                ));
            }
        }
        let dimension = if d[3] > 1 { 4 } else { 3 };
        let a = &self.index_to_lps.0;
        let mut b = String::from(
            "NRRD0004\n# Complete NRRD file format specification at:\n# http://teem.sourceforge.net/nrrd/format.html\n",
        );
        b.push_str(&format!(
            "type: {typ}\ndimension: {dimension}\nspace: left-posterior-superior\n"
        ));
        let mut sizes: Vec<String> = d[..3].iter().map(|v| v.to_string()).collect();
        let mut directions: Vec<String> = (0..3)
            .map(|column| {
                format!(
                    "({},{},{})",
                    format_number(a[column]),
                    format_number(a[4 + column]),
                    format_number(a[8 + column])
                )
            })
            .collect();
        let mut kinds = vec!["domain".to_string(); 3];
        if dimension == 4 {
            let kind = if self.options.fourth_axis_kind.is_empty() {
                "list".to_string()
            } else {
                self.options.fourth_axis_kind.clone()
            };
            if self.list_first {
                sizes.insert(0, d[3].to_string());
                directions.insert(0, "none".to_string());
                kinds.insert(0, kind);
            } else {
                sizes.push(d[3].to_string());
                directions.push("none".to_string());
                kinds.push(kind);
            }
        }
        b.push_str(&format!(
            "sizes: {}\nspace directions: {}\nkinds: {}\n",
            sizes.join(" "),
            directions.join(" "),
            kinds.join(" ")
        ));
        if width > 1 {
            b.push_str("endian: little\n");
        }
        let encoding = if self.options.raw { "raw" } else { "gzip" };
        b.push_str(&format!(
            "encoding: {encoding}\nspace origin: ({},{},{})\n",
            format_number(a[3]),
            format_number(a[7]),
            format_number(a[11])
        ));
        for (key, value) in &self.options.key_values {
            b.push_str(&format!(
                "{}:={}\n",
                escape_key_value(key),
                escape_key_value(value)
            ));
        }
        b.push('\n');
        Ok(b)
    }
}
pub(crate) fn write_encoded<'a>(
    header: String,
    raw: bool,
    writes: impl Iterator<Item = &'a [u8]>,
) -> Vec<u8> {
    let mut out = header.into_bytes();
    if raw {
        for chunk in writes {
            out.extend_from_slice(chunk);
        }
        return out;
    }
    let mut compressor = DeflateWriter::new(Container::Gzip);
    for chunk in writes {
        compressor.write(chunk);
    }
    out.extend_from_slice(&compressor.finish());
    out
}
pub fn write(v: &Volume, options: &WriteOptions) -> Result<Vec<u8>, Error> {
    if v.scaling.applied {
        return Err(wrap(Sentinel::Unsupported, "value scaling"));
    }
    let header = HeaderSpec {
        datatype: v.header.datatype,
        dimensions: v.dimensions,
        index_to_lps: v.geometry.index_to_lps,
        options,
        list_first: false,
    }
    .header()?;
    Ok(write_encoded(
        header,
        options.raw,
        v.payload().chunks(nifti::PAYLOAD_CHUNK_BYTES),
    ))
}
#[derive(Debug, Clone, PartialEq)]
pub enum PlanWriteError {
    Nrrd(Error),
    Export(nifti::export::Error),
}
impl std::fmt::Display for PlanWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanWriteError::Nrrd(e) => e.fmt(f),
            PlanWriteError::Export(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for PlanWriteError {}
pub fn write_plan(
    plan: &nifti::export::Plan<'_>,
    options: &WriteOptions,
) -> Result<(Vec<u8>, nifti::export::Report), PlanWriteError> {
    let layout = plan.layout();
    if !layout.identity_scaling() {
        return Err(PlanWriteError::Nrrd(wrap(
            Sentinel::Unsupported,
            format!(
                "value scaling (slope {}, intercept {})",
                numtext::format_float(layout.scaling_slope, 'g', -1),
                numtext::format_float(layout.scaling_intercept, 'g', -1)
            ),
        )));
    }
    if layout.no_spatial_transform {
        return Err(PlanWriteError::Nrrd(wrap(
            Sentinel::NoPatientSpace,
            "image grid without patient geometry",
        )));
    }
    let header = HeaderSpec {
        datatype: layout.datatype,
        dimensions: layout.dimensions,
        index_to_lps: layout.index_to_lps,
        options,
        list_first: false,
    }
    .header()
    .map_err(PlanWriteError::Nrrd)?;
    let mut chunks = nifti::export::Chunks::default();
    let mut report = plan
        .write_voxels(&mut chunks)
        .map_err(PlanWriteError::Export)?;
    let out = write_encoded(header, options.raw, chunks.0.iter().map(Vec::as_slice));
    report.bytes_written = out.len() as i64;
    if !options.raw {
        report.compression = nifti::export::COMPRESSION_GZIP;
    }
    Ok((out, report))
}
