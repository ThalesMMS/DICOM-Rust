use super::{Geometry, Header, Volume};
use crate::textfmt::{float_v, quote};
use sha2::{Digest, Sha256};
pub fn floats_v(values: &[f64]) -> String {
    let parts: Vec<String> = values.iter().map(|&v| float_v(v)).collect();
    format!("[{}]", parts.join(" "))
}
#[doc(hidden)]
pub fn ints_v(values: &[i64]) -> String {
    let parts: Vec<String> = values.iter().map(|v| v.to_string()).collect();
    format!("[{}]", parts.join(" "))
}
#[doc(hidden)]
pub fn strings_q(values: &[String]) -> String {
    let parts: Vec<String> = values.iter().map(|v| quote(v)).collect();
    format!("[{}]", parts.join(" "))
}
pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
#[doc(hidden)]
pub fn header_line(h: &Header) -> String {
    let mut b = String::new();
    let srow: Vec<String> = h.s_row.iter().map(|row| floats_v(row)).collect();
    b.push_str(&format!(
        "header v={} paired={} be={} dim={} pixdim={} dt={} bitpix={} vox={} scl={},{} cal={},{} sdur={} toff={} slice={},{},{} diminfo={} units={} intent={},{} q={} s={} quat={},{},{} qoff={} srow=[{}] ext={}\n",
        h.version,
        h.paired,
        h.big_endian,
        ints_v(&h.dim),
        floats_v(&h.pix_dim),
        h.datatype,
        h.bit_pix,
        float_v(h.voxel_offset),
        float_v(h.scl_slope),
        float_v(h.scl_inter),
        float_v(h.cal_max),
        float_v(h.cal_min),
        float_v(h.slice_duration),
        float_v(h.t_offset),
        h.slice_start,
        h.slice_end,
        h.slice_code,
        h.dim_info,
        h.xyzt_units,
        h.intent_code,
        floats_v(&h.intent_params),
        h.qform_code.0,
        h.sform_code.0,
        float_v(h.quatern_b),
        float_v(h.quatern_c),
        float_v(h.quatern_d),
        floats_v(&h.q_offset),
        srow.join(" "),
        h.extended
    ));
    b
}
#[doc(hidden)]
pub fn geometry_line(g: &Geometry) -> String {
    let mut b = String::new();
    b.push_str(&format!(
        "geom src={} code={} q={} s={} ras={} lps={} spacing={} left={} agree={} corrected={} dev={} warn={}\n",
        g.source.as_str(),
        g.code.0,
        g.qform_code.0,
        g.sform_code.0,
        floats_v(&g.index_to_ras.0),
        floats_v(&g.index_to_lps.0),
        floats_v(&g.spacing),
        g.left_handed,
        g.qform_sform_agree,
        g.sform_corrected,
        float_v(g.sform_deviation_degrees),
        strings_q(&g.warnings)
    ));
    b
}
impl Volume {
    pub fn dump(&self) -> String {
        let h = &self.header;
        let g = &self.geometry;
        let mut b = String::new();
        b.push_str(&format!(
            "dims={} len={} bpv={}\n",
            ints_v(&self.dimensions),
            self.len(),
            self.bytes_per_voxel()
        ));
        b.push_str(&header_line(h));
        b.push_str(&geometry_line(g));
        b.push_str(&format!(
            "scaling={},{},{} warnings={}\n",
            float_v(self.scaling.slope),
            float_v(self.scaling.intercept),
            self.scaling.applied,
            strings_q(&self.warnings)
        ));
        b.push_str(&format!("payload={}\n", sha256_hex(self.payload())));
        for index in 0..self.len().min(6) {
            b.push_str(&format!(
                "{}/{} ",
                float_v(self.stored(index)),
                float_v(self.value(index))
            ));
        }
        b.push('\n');
        b
    }
}
#[doc(hidden)]
pub fn error_identities(err: &super::Error) -> Vec<&'static str> {
    use super::Sentinel as S;
    [
        ("n.InvalidSource", S::InvalidSource),
        ("n.LimitExceeded", S::LimitExceeded),
        ("n.NotNIfTI", S::NotNifti),
        ("n.MalformedHeader", S::MalformedHeader),
        ("n.UnsupportedDatatype", S::UnsupportedDatatype),
        ("n.UnsupportedDimensions", S::UnsupportedDimensions),
        ("n.NoSpatialTransform", S::NoSpatialTransform),
        ("n.InvalidTransform", S::InvalidTransform),
        ("n.NonOrthogonalSForm", S::NonOrthogonalSForm),
        ("n.TruncatedData", S::TruncatedData),
        ("n.PairedFile", S::PairedFile),
    ]
    .into_iter()
    .filter(|(_, sentinel)| err.is(*sentinel))
    .map(|(name, _)| name)
    .collect()
}
