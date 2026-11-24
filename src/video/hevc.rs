use super::nal::{BitReader, rbsp};
use super::{Error, malformed};
use std::collections::HashMap;
use std::rc::Rc;
pub(crate) const HEVC_NAL_RADL_N: u8 = 6;
const HEVC_NAL_RASL_R: u8 = 9;
const HEVC_NAL_RASL_N: u8 = 8;
const HEVC_NAL_BLA_W_LP: u8 = 16;
const HEVC_NAL_IDR_W_RADL: u8 = 19;
const HEVC_NAL_IDR_N_LP: u8 = 20;
const HEVC_NAL_IRAP_MAX: u8 = 23;
pub(crate) const HEVC_NAL_VPS: u8 = 32;
pub(crate) const HEVC_NAL_SPS: u8 = 33;
pub(crate) const HEVC_NAL_PPS: u8 = 34;
pub(crate) const HEVC_NAL_AUD: u8 = 35;
pub(crate) const HEVC_NAL_EOS: u8 = 36;
pub(crate) const HEVC_NAL_EOB: u8 = 37;
pub(crate) const HEVC_NAL_FD: u8 = 38;
pub(crate) const HEVC_NAL_SEI_PREFIX: u8 = 39;
pub(crate) fn hevc_type(nal: &[u8]) -> u8 {
    (nal[0] >> 1) & 0x3f
}
pub(crate) fn hevc_irap(t: u8) -> bool {
    (HEVC_NAL_BLA_W_LP..=HEVC_NAL_IRAP_MAX).contains(&t)
}
pub(crate) fn hevc_idr(t: u8) -> bool {
    t == HEVC_NAL_IDR_W_RADL || t == HEVC_NAL_IDR_N_LP
}
pub(crate) fn hevc_rasl(t: u8) -> bool {
    t == HEVC_NAL_RASL_N || t == HEVC_NAL_RASL_R
}
pub(crate) struct HevcSps {
    pub id: u32,
    pub ptl: [u8; 12],
    pub max_sub_layers: u32,
    pub temporal_id_nesting: bool,
    pub chroma_format: u32,
    pub separate_colour_plane: bool,
    pub bit_depth_luma: u32,
    pub bit_depth_chroma: u32,
    pub log2_max_poc_lsb: u32,
    pub width: i64,
    pub height: i64,
}
pub(crate) struct HevcPps {
    pub id: u32,
    pub sps_id: u32,
    pub output_flag_present: bool,
    pub num_extra_slice_header_bits: usize,
}
pub(crate) fn parse_hevc_sps(nal: &[u8]) -> Result<HevcSps, Error> {
    if nal.len() < 16 {
        return Err(malformed("HEVC SPS too short"));
    }
    let data = rbsp(&nal[2..]);
    let mut r = BitReader::new(&data);
    r.skip(4);
    let max_sub_layers_minus1 = r.u(3) as usize;
    let temporal_id_nesting = r.flag();
    let mut ptl = [0u8; 12];
    for b in &mut ptl {
        *b = r.u(8) as u8;
    }
    let (mut profile_present, mut level_present) = ([false; 8], [false; 8]);
    for i in 0..max_sub_layers_minus1 {
        profile_present[i] = r.flag();
        level_present[i] = r.flag();
    }
    if max_sub_layers_minus1 > 0 {
        r.skip(2 * (8 - max_sub_layers_minus1));
    }
    for i in 0..max_sub_layers_minus1 {
        if profile_present[i] {
            r.skip(88);
        }
        if level_present[i] {
            r.skip(8);
        }
    }
    let id = r.ue_max(15, "sps_seq_parameter_set_id");
    let chroma_format = r.ue_max(3, "chroma_format_idc");
    let mut separate_colour_plane = false;
    if chroma_format == 3 {
        separate_colour_plane = r.flag();
    }
    let mut width = i64::from(r.ue_max(1 << 16, "pic_width_in_luma_samples"));
    let mut height = i64::from(r.ue_max(1 << 16, "pic_height_in_luma_samples"));
    if r.flag() {
        let (left, right, top, bottom) = (r.ue(), r.ue(), r.ue(), r.ue());
        let (mut sub_w, mut sub_h) = (1i64, 1i64);
        if !separate_colour_plane {
            match chroma_format {
                1 => (sub_w, sub_h) = (2, 2),
                2 => sub_w = 2,
                _ => {}
            }
        }
        width -= i64::from(left.wrapping_add(right)) * sub_w;
        height -= i64::from(top.wrapping_add(bottom)) * sub_h;
    }
    let bit_depth_luma = r.ue_max(8, "bit_depth_luma_minus8").wrapping_add(8);
    let bit_depth_chroma = r.ue_max(8, "bit_depth_chroma_minus8").wrapping_add(8);
    let log2_max_poc_lsb = r
        .ue_max(12, "log2_max_pic_order_cnt_lsb_minus4")
        .wrapping_add(4);
    if let Some(err) = r.err.take() {
        return Err(err);
    }
    if width <= 0 || height <= 0 {
        return Err(malformed(format!("HEVC SPS picture size {width}x{height}")));
    }
    Ok(HevcSps {
        id,
        ptl,
        max_sub_layers: max_sub_layers_minus1 as u32 + 1,
        temporal_id_nesting,
        chroma_format,
        separate_colour_plane,
        bit_depth_luma,
        bit_depth_chroma,
        log2_max_poc_lsb,
        width,
        height,
    })
}
pub(crate) fn parse_hevc_pps(nal: &[u8]) -> Result<HevcPps, Error> {
    let data = rbsp(&nal[2..]);
    let mut r = BitReader::new(&data);
    let id = r.ue_max(63, "pps_pic_parameter_set_id");
    let sps_id = r.ue_max(15, "pps_seq_parameter_set_id");
    r.skip(1);
    let output_flag_present = r.flag();
    let num_extra_slice_header_bits = r.u(3) as usize;
    match r.err {
        Some(err) => Err(err),
        None => Ok(HevcPps {
            id,
            sps_id,
            output_flag_present,
            num_extra_slice_header_bits,
        }),
    }
}
pub(crate) struct HevcSlice {
    pub nal_type: u8,
    pub temporal_id: i32,
    pub poc_lsb: u32,
    pub sps: Rc<HevcSps>,
}
pub(crate) fn parse_hevc_slice(
    nal: &[u8],
    spss: &HashMap<u32, Rc<HevcSps>>,
    ppss: &HashMap<u32, HevcPps>,
) -> Result<HevcSlice, Error> {
    let t = hevc_type(nal);
    let data = rbsp(&nal[2..nal.len().min(64)]);
    let mut r = BitReader::new(&data);
    let temporal_id = i32::from(nal[1] & 7) - 1;
    if temporal_id < 0 {
        return Err(malformed("HEVC nuh_temporal_id_plus1 is 0"));
    }
    r.skip(1);
    if hevc_irap(t) {
        r.skip(1);
    }
    let pps_id = r.ue();
    if let Some(err) = r.err.take() {
        return Err(err);
    }
    let Some(pps) = ppss.get(&pps_id) else {
        return Err(malformed(format!("slice refers to missing PPS {pps_id}")));
    };
    let Some(sps) = spss.get(&pps.sps_id) else {
        return Err(malformed(format!(
            "PPS {pps_id} refers to missing SPS {}",
            pps.sps_id
        )));
    };
    r.skip(pps.num_extra_slice_header_bits);
    r.ue();
    if pps.output_flag_present {
        r.skip(1);
    }
    if sps.separate_colour_plane {
        r.skip(2);
    }
    let mut poc_lsb = 0;
    if !hevc_idr(t) {
        poc_lsb = r.u(sps.log2_max_poc_lsb);
    }
    match r.err {
        Some(err) => Err(err),
        None => Ok(HevcSlice {
            nal_type: t,
            temporal_id,
            poc_lsb,
            sps: Rc::clone(sps),
        }),
    }
}
#[derive(Default)]
pub(crate) struct HevcPoc {
    prev_msb: i64,
    prev_lsb: i64,
    pub after_eos: bool,
    started: bool,
}
impl HevcPoc {
    pub fn next(&mut self, sl: &HevcSlice) -> (i64, bool) {
        let max_lsb = 1i64 << sl.sps.log2_max_poc_lsb;
        let lsb = i64::from(sl.poc_lsb);
        let t = sl.nal_type;
        let no_rasl_output = hevc_irap(t)
            && (hevc_idr(t) || t < HEVC_NAL_IDR_W_RADL || !self.started || self.after_eos);
        (self.started, self.after_eos) = (true, false);
        let mut msb = 0i64;
        if !no_rasl_output {
            msb = self.prev_msb;
            if lsb < self.prev_lsb && self.prev_lsb - lsb >= max_lsb / 2 {
                msb += max_lsb;
            } else if lsb > self.prev_lsb && lsb - self.prev_lsb > max_lsb / 2 {
                msb -= max_lsb;
            }
        }
        let sub_layer_non_ref = t <= 14 && t.is_multiple_of(2);
        if sl.temporal_id == 0
            && !hevc_rasl(t)
            && t != HEVC_NAL_RADL_N
            && t != HEVC_NAL_RADL_N + 1
            && !sub_layer_non_ref
        {
            (self.prev_msb, self.prev_lsb) = (msb, lsb);
        }
        (msb + lsb, no_rasl_output)
    }
}
pub(crate) fn hevc_codec_string(entry: &str, ptl: &[u8; 12]) -> String {
    let space = ptl[0] >> 6;
    let tier = (ptl[0] >> 5) & 1;
    let profile = ptl[0] & 0x1f;
    let compat = u32::from_be_bytes([ptl[1], ptl[2], ptl[3], ptl[4]]);
    let mut s = format!("{entry}.");
    if space > 0 {
        s.push(char::from(b'A' + space - 1));
    }
    s.push_str(&format!("{profile}.{:X}", compat.reverse_bits()));
    s.push_str(if tier == 1 { ".H" } else { ".L" });
    s.push_str(&ptl[11].to_string());
    let constraints = &ptl[5..11];
    let last = constraints
        .iter()
        .rposition(|&b| b != 0)
        .map_or(0, |i| i + 1);
    for b in &constraints[..last] {
        s.push_str(&format!(".{b:X}"));
    }
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codec_string() {
        let ptl = [1, 0x60, 0, 0, 0, 0xb0, 0, 0, 0, 0, 0, 93];
        assert_eq!(hevc_codec_string("hvc1", &ptl), "hvc1.1.6.L93.B0");
        let ptl = [0x62, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 153];
        assert_eq!(hevc_codec_string("hev1", &ptl), "hev1.A2.4.H153");
    }
}
