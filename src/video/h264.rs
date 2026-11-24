use super::nal::{BitReader, rbsp};
use super::{Error, malformed, unsupported_stream};
use std::collections::HashMap;
use std::rc::Rc;
pub(crate) const H264_NAL_SLICE: u8 = 1;
pub(crate) const H264_NAL_IDR: u8 = 5;
pub(crate) const H264_NAL_SEI: u8 = 6;
pub(crate) const H264_NAL_SPS: u8 = 7;
pub(crate) const H264_NAL_PPS: u8 = 8;
pub(crate) const H264_NAL_AUD: u8 = 9;
pub(crate) const H264_NAL_END_SEQ: u8 = 10;
pub(crate) const H264_NAL_END_STREAM: u8 = 11;
pub(crate) const H264_NAL_FILLER: u8 = 12;
pub(crate) const H264_NAL_PREFIX: u8 = 14;
pub(crate) const H264_NAL_SUBSET_SPS: u8 = 15;
pub(crate) struct H264Sps<'a> {
    pub id: u32,
    pub raw: &'a [u8],
    pub profile: u8,
    pub compat: u8,
    pub level: u8,
    pub chroma_format: u32,
    pub separate_colour_plane: bool,
    pub bit_depth_luma: u32,
    pub bit_depth_chroma: u32,
    pub log2_max_frame_num: u32,
    pub poc_type: u32,
    pub log2_max_poc_lsb: u32,
    pub delta_pic_order_always_zero: bool,
    pub offset_for_non_ref_pic: i32,
    pub offset_for_top_to_bottom_field: i32,
    pub offset_for_ref_frame: Vec<i32>,
    pub frame_mbs_only: bool,
    pub width: i64,
    pub height: i64,
    pub vui_bit_offset: usize,
}
pub(crate) struct H264Pps {
    pub id: u32,
    pub sps_id: u32,
    pub bottom_field_pic_order_present: bool,
}
pub(crate) fn h264_high_profile(p: u8) -> bool {
    matches!(
        p,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    )
}
fn skip_scaling_list(r: &mut BitReader<'_>, size: usize) {
    let (mut last, mut next) = (8i32, 8i32);
    for _ in 0..size {
        if next != 0 {
            next = last.wrapping_add(r.se()).wrapping_add(256) % 256;
        }
        if next != 0 {
            last = next;
        }
    }
}
pub(crate) fn parse_h264_sps(nal: &[u8]) -> Result<H264Sps<'_>, Error> {
    if nal.len() < 4 {
        return Err(malformed("H.264 SPS too short"));
    }
    let data = rbsp(&nal[1..]);
    let mut r = BitReader::new(&data);
    let profile = r.u(8) as u8;
    let compat = r.u(8) as u8;
    let level = r.u(8) as u8;
    let id = r.ue_max(31, "seq_parameter_set_id");
    let (mut chroma_format, mut separate_colour_plane) = (1u32, false);
    let (mut bit_depth_luma, mut bit_depth_chroma) = (8u32, 8u32);
    if h264_high_profile(profile) {
        chroma_format = r.ue_max(3, "chroma_format_idc");
        if chroma_format == 3 {
            separate_colour_plane = r.flag();
        }
        bit_depth_luma = r.ue_max(6, "bit_depth_luma_minus8").wrapping_add(8);
        bit_depth_chroma = r.ue_max(6, "bit_depth_chroma_minus8").wrapping_add(8);
        r.skip(1);
        if r.flag() {
            let n = if chroma_format == 3 { 12 } else { 8 };
            for i in 0..n {
                if r.flag() {
                    skip_scaling_list(&mut r, if i >= 6 { 64 } else { 16 });
                }
            }
        }
    }
    let log2_max_frame_num = r.ue_max(12, "log2_max_frame_num_minus4").wrapping_add(4);
    let poc_type = r.ue_max(2, "pic_order_cnt_type");
    let mut log2_max_poc_lsb = 0u32;
    let mut delta_pic_order_always_zero = false;
    let (mut offset_for_non_ref_pic, mut offset_for_top_to_bottom_field) = (0i32, 0i32);
    let mut offset_for_ref_frame = Vec::new();
    match poc_type {
        0 => {
            log2_max_poc_lsb = r
                .ue_max(12, "log2_max_pic_order_cnt_lsb_minus4")
                .wrapping_add(4);
        }
        1 => {
            delta_pic_order_always_zero = r.flag();
            offset_for_non_ref_pic = r.se();
            offset_for_top_to_bottom_field = r.se();
            let n = r.ue_max(255, "num_ref_frames_in_pic_order_cnt_cycle");
            let mut i = 0;
            while i < n && r.err.is_none() {
                offset_for_ref_frame.push(r.se());
                i += 1;
            }
        }
        _ => {}
    }
    r.ue();
    r.skip(1);
    let width_mbs = r.ue_max(1 << 16, "pic_width_in_mbs_minus1").wrapping_add(1);
    let height_units = r
        .ue_max(1 << 16, "pic_height_in_map_units_minus1")
        .wrapping_add(1);
    let frame_mbs_only = r.flag();
    if !frame_mbs_only {
        r.skip(1);
    }
    r.skip(1);
    let frame_height_mul: u32 = if frame_mbs_only { 1 } else { 2 };
    let mut width = i64::from(width_mbs) * 16;
    let mut height = i64::from(frame_height_mul.wrapping_mul(height_units)) * 16;
    if r.flag() {
        let (left, right, top, bottom) = (r.ue(), r.ue(), r.ue(), r.ue());
        let (mut crop_x, mut crop_y) = (1i64, i64::from(frame_height_mul));
        if chroma_format != 0 && !separate_colour_plane {
            let (sub_w, sub_h) = match chroma_format {
                2 => (2, 1),
                3 => (1, 1),
                _ => (2, 2),
            };
            (crop_x, crop_y) = (sub_w, sub_h * i64::from(frame_height_mul));
        }
        width -= i64::from(left.wrapping_add(right)) * crop_x;
        height -= i64::from(top.wrapping_add(bottom)) * crop_y;
    }
    if let Some(err) = r.err.take() {
        return Err(err);
    }
    if width <= 0 || height <= 0 || width >= 1 << 16 || height >= 1 << 16 {
        return Err(malformed(format!(
            "H.264 SPS picture size {width}x{height}"
        )));
    }
    Ok(H264Sps {
        id,
        raw: nal,
        profile,
        compat,
        level,
        chroma_format,
        separate_colour_plane,
        bit_depth_luma,
        bit_depth_chroma,
        log2_max_frame_num,
        poc_type,
        log2_max_poc_lsb,
        delta_pic_order_always_zero,
        offset_for_non_ref_pic,
        offset_for_top_to_bottom_field,
        offset_for_ref_frame,
        frame_mbs_only,
        width,
        height,
        vui_bit_offset: r.pos,
    })
}
pub(crate) fn parse_h264_pps(nal: &[u8]) -> Result<H264Pps, Error> {
    let data = rbsp(&nal[1..]);
    let mut r = BitReader::new(&data);
    let id = r.ue_max(255, "pic_parameter_set_id");
    let sps_id = r.ue_max(31, "seq_parameter_set_id");
    r.skip(1);
    let bottom_field_pic_order_present = r.flag();
    match r.err {
        Some(err) => Err(err),
        None => Ok(H264Pps {
            id,
            sps_id,
            bottom_field_pic_order_present,
        }),
    }
}
pub(crate) struct H264Slice<'a> {
    pub first_mb: u32,
    pub frame_num: u32,
    pub idr: bool,
    pub ref_idc: u32,
    pub poc_lsb: u32,
    pub delta_bottom: i32,
    pub delta_pic_order: [i32; 2],
    pub sps: Rc<H264Sps<'a>>,
}
pub(crate) fn parse_h264_slice<'a>(
    nal: &[u8],
    spss: &HashMap<u32, Rc<H264Sps<'a>>>,
    ppss: &HashMap<u32, H264Pps>,
) -> Result<H264Slice<'a>, Error> {
    let data = rbsp(&nal[1..nal.len().min(64)]);
    let mut r = BitReader::new(&data);
    let first_mb = r.ue();
    r.ue();
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
    let mut sl = H264Slice {
        first_mb,
        frame_num: 0,
        idr: nal[0] & 0x1f == H264_NAL_IDR,
        ref_idc: u32::from(nal[0] >> 5) & 3,
        poc_lsb: 0,
        delta_bottom: 0,
        delta_pic_order: [0; 2],
        sps: Rc::clone(sps),
    };
    if sps.separate_colour_plane {
        r.skip(2);
    }
    sl.frame_num = r.u(sps.log2_max_frame_num);
    if !sps.frame_mbs_only && r.flag() {
        return Err(unsupported_stream(
            "H.264 field pictures (interlaced coding)",
        ));
    }
    if sl.idr {
        r.ue();
    }
    match sps.poc_type {
        0 => {
            sl.poc_lsb = r.u(sps.log2_max_poc_lsb);
            if pps.bottom_field_pic_order_present {
                sl.delta_bottom = r.se();
            }
        }
        1 if !sps.delta_pic_order_always_zero => {
            sl.delta_pic_order[0] = r.se();
            if pps.bottom_field_pic_order_present {
                sl.delta_pic_order[1] = r.se();
            }
        }
        _ => {}
    }
    match r.err {
        Some(err) => Err(err),
        None => Ok(sl),
    }
}
#[derive(Default)]
pub(crate) struct H264Poc {
    prev_msb: i64,
    prev_lsb: i64,
    prev_frame_num: u32,
    prev_frame_num_offset: i64,
}
impl H264Poc {
    pub fn next(&mut self, sl: &H264Slice<'_>) -> i64 {
        let sps = &sl.sps;
        let max_frame_num = 1i64 << sps.log2_max_frame_num;
        let frame_num_offset = if sl.idr {
            0
        } else if self.prev_frame_num > sl.frame_num {
            self.prev_frame_num_offset + max_frame_num
        } else {
            self.prev_frame_num_offset
        };
        let frame_num = i64::from(sl.frame_num);
        let poc;
        match sps.poc_type {
            0 => {
                if sl.idr {
                    (self.prev_msb, self.prev_lsb) = (0, 0);
                }
                let max_lsb = 1i64 << sps.log2_max_poc_lsb;
                let lsb = i64::from(sl.poc_lsb);
                let mut msb = self.prev_msb;
                if lsb < self.prev_lsb && self.prev_lsb - lsb >= max_lsb / 2 {
                    msb += max_lsb;
                } else if lsb > self.prev_lsb && lsb - self.prev_lsb > max_lsb / 2 {
                    msb -= max_lsb;
                }
                let top = msb + lsb;
                poc = top.min(top + i64::from(sl.delta_bottom));
                if sl.ref_idc != 0 {
                    (self.prev_msb, self.prev_lsb) = (msb, lsb);
                }
            }
            1 => {
                let offsets = &sps.offset_for_ref_frame;
                let n = offsets.len() as i64;
                let mut abs = if n != 0 {
                    frame_num_offset + frame_num
                } else {
                    0
                };
                if sl.ref_idc == 0 && abs > 0 {
                    abs -= 1;
                }
                let mut expected = 0i64;
                if abs > 0 {
                    let per_cycle: i64 = offsets.iter().map(|&d| i64::from(d)).sum();
                    let (cycle, in_cycle) = ((abs - 1) / n, (abs - 1) % n);
                    expected = cycle * per_cycle;
                    for &d in &offsets[..=in_cycle as usize] {
                        expected += i64::from(d);
                    }
                }
                if sl.ref_idc == 0 {
                    expected += i64::from(sps.offset_for_non_ref_pic);
                }
                let top = expected + i64::from(sl.delta_pic_order[0]);
                let bottom = top
                    + i64::from(sps.offset_for_top_to_bottom_field)
                    + i64::from(sl.delta_pic_order[1]);
                poc = top.min(bottom);
                self.prev_frame_num_offset = frame_num_offset;
            }
            _ => {
                poc = if sl.idr {
                    0
                } else if sl.ref_idc == 0 {
                    2 * (frame_num_offset + frame_num) - 1
                } else {
                    2 * (frame_num_offset + frame_num)
                };
                self.prev_frame_num_offset = frame_num_offset;
            }
        }
        self.prev_frame_num = sl.frame_num;
        poc
    }
}
