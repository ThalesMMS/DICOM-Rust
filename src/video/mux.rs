use super::extract::Limits;
use super::h264::{
    H264_NAL_AUD, H264_NAL_END_SEQ, H264_NAL_END_STREAM, H264_NAL_FILLER, H264_NAL_IDR,
    H264_NAL_PPS, H264_NAL_PREFIX, H264_NAL_SEI, H264_NAL_SLICE, H264_NAL_SPS, H264_NAL_SUBSET_SPS,
    H264Poc, H264Sps, h264_high_profile, parse_h264_pps, parse_h264_slice, parse_h264_sps,
};
use super::hevc::{
    HEVC_NAL_AUD, HEVC_NAL_EOB, HEVC_NAL_EOS, HEVC_NAL_FD, HEVC_NAL_PPS, HEVC_NAL_SEI_PREFIX,
    HEVC_NAL_SPS, HEVC_NAL_VPS, HevcPoc, HevcSps, hevc_codec_string, hevc_idr, hevc_irap,
    hevc_rasl, hevc_type, parse_hevc_pps, parse_hevc_slice, parse_hevc_sps,
};
use super::nal::split_annex_b;
use super::{CODEC_H264, CODEC_HEVC, Error, Kind, malformed, unsupported_stream};
use crate::fmath;
use crate::textfmt::quote;
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;
pub const DEFAULT_MAX_SAMPLES_PER_FRAGMENT: i64 = 300;
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MuxOptions {
    pub codec: &'static str,
    pub frame_rate: f64,
    pub max_samples_per_fragment: i64,
    pub limits: Limits,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MuxInfo {
    pub codec: &'static str,
    pub codec_string: String,
    pub sample_entry: &'static str,
    pub width: i64,
    pub height: i64,
    pub samples: i64,
    pub key_frames: i64,
    pub fragments: i64,
    pub dropped_leading: i64,
    pub timescale: u32,
    pub sample_duration: u32,
    pub reordered: bool,
    pub size: i64,
}
struct MuxSample<'a> {
    nals: Vec<&'a [u8]>,
    sync: bool,
    poc: i64,
    cvs: i64,
    pres: usize,
}
struct ParamSet<'a> {
    id: u32,
    raw: &'a [u8],
}
#[derive(Default)]
struct MuxStream<'a> {
    hevc_codec: bool,
    samples: Vec<MuxSample<'a>>,
    in_band: bool,
    sps: Vec<ParamSet<'a>>,
    pps: Vec<ParamSet<'a>>,
    vps: Vec<ParamSet<'a>>,
    width: i64,
    height: i64,
    h264: Option<Rc<H264Sps<'a>>>,
    hevc: Option<Rc<HevcSps>>,
    dropped: i64,
}
pub fn mux_fragmented_mp4(
    dst: &mut dyn Write,
    stream: &[u8],
    opts: MuxOptions,
) -> Result<MuxInfo, Error> {
    if opts.codec != CODEC_H264 && opts.codec != CODEC_HEVC {
        return Err(unsupported_stream(format!(
            "MP4 muxing supports H.264 and HEVC, not {}",
            quote(opts.codec)
        )));
    }
    let rate = opts.frame_rate;
    if rate.is_nan() || rate <= 0.0 || rate.is_infinite() || rate > 1000.0 {
        return Err(Error::bare(Kind::FrameRateRequired));
    }
    let limits = opts.limits.normalized();
    if stream.len() as i64 > limits.max_bytes {
        return Err(Error::new(
            Kind::MediaTooLarge,
            format!("limit {} bytes", limits.max_bytes),
        ));
    }
    let per_fragment = if opts.max_samples_per_fragment <= 0 {
        DEFAULT_MAX_SAMPLES_PER_FRAGMENT as usize
    } else {
        usize::try_from(opts.max_samples_per_fragment).unwrap_or(usize::MAX)
    };
    let nals = split_annex_b(stream, limits.max_fragments)?;
    let mut s = if opts.codec == CODEC_H264 {
        analyze_h264(&nals)?
    } else {
        analyze_hevc(&nals)?
    };
    if s.samples.is_empty() {
        return Err(malformed("no decodable pictures (no key frame)"));
    }
    s.check_config()?;
    let reordered = s.order_presentation();
    let (timescale, duration) = media_timing(rate);
    let (sample_entry, codec_string) = s.sample_entry();
    let mut info = MuxInfo {
        codec: opts.codec,
        codec_string,
        sample_entry,
        width: s.width,
        height: s.height,
        samples: s.samples.len() as i64,
        key_frames: s.samples.iter().filter(|sm| sm.sync).count() as i64,
        dropped_leading: s.dropped,
        timescale,
        sample_duration: duration,
        reordered,
        ..MuxInfo::default()
    };
    let mut w = CountingWriter { dst, n: 0 };
    w.write(&s.init_segment(&info)?)?;
    let mut start = 0;
    while start < s.samples.len() {
        let mut end = start + 1;
        while end < s.samples.len() && end - start < per_fragment && !s.samples[end].sync {
            end += 1;
        }
        info.fragments += 1;
        s.write_fragment(&mut w, info.fragments as u32, start, end, duration)?;
        start = end;
    }
    info.size = w.n;
    Ok(info)
}
fn media_timing(rate: f64) -> (u32, u32) {
    let to_u32 = |v: f64| fmath::f64_to_i64(v) as u32;
    let n = (rate * 1.001).round();
    if n >= 1.0 && (rate * 1.001 - n).abs() < 1e-3 && (rate - n).abs() > 1e-3 {
        return (to_u32(n).wrapping_mul(1000), 1001);
    }
    let d = 90000.0 / rate;
    if (d - d.round()).abs() < 1e-6 {
        return (90000, to_u32(d.round()));
    }
    (to_u32((rate * 1000.0).round()), 1000)
}
fn add_param_set<'a>(list: &mut Vec<ParamSet<'a>>, id: u32, raw: &'a [u8]) -> bool {
    if let Some(p) = list.iter().find(|p| p.id == id) {
        return p.raw == raw;
    }
    list.push(ParamSet { id, raw });
    true
}
fn analyze_h264<'a>(nals: &[&'a [u8]]) -> Result<MuxStream<'a>, Error> {
    let mut s = MuxStream::default();
    let mut spss: HashMap<u32, Rc<H264Sps<'a>>> = HashMap::new();
    let mut ppss = HashMap::new();
    let mut poc = H264Poc::default();
    let mut cur: Option<MuxSample<'a>> = None;
    let mut pending: Vec<&'a [u8]> = Vec::new();
    let mut started = false;
    let mut cvs = -1i64;
    for &nal in nals {
        if nal[0] & 0x80 != 0 {
            return Err(malformed("forbidden_zero_bit set"));
        }
        let t = nal[0] & 0x1f;
        match t {
            H264_NAL_SPS => {
                let sps = Rc::new(parse_h264_sps(nal)?);
                if !add_param_set(&mut s.sps, sps.id, nal) {
                    s.in_band = true;
                }
                if s.h264.is_none() {
                    (s.width, s.height) = (sps.width, sps.height);
                    s.h264 = Some(Rc::clone(&sps));
                }
                spss.insert(sps.id, sps);
            }
            H264_NAL_PPS => {
                let pps = parse_h264_pps(nal)?;
                if !add_param_set(&mut s.pps, pps.id, nal) {
                    s.in_band = true;
                }
                ppss.insert(pps.id, pps);
            }
            _ => {}
        }
        if t == H264_NAL_SLICE || t == H264_NAL_IDR {
            let sl = parse_h264_slice(nal, &spss, &ppss)?;
            if sl.first_mb == 0 || cur.is_none() {
                s.samples.extend(cur.take());
                if sl.idr {
                    cvs += 1;
                    started = true;
                }
                if !started {
                    s.dropped += 1;
                    pending.clear();
                    continue;
                }
                cur = Some(MuxSample {
                    nals: std::mem::take(&mut pending),
                    sync: sl.idr,
                    poc: poc.next(&sl),
                    cvs,
                    pres: 0,
                });
            }
            if let Some(sample) = &mut cur {
                sample.nals.push(nal);
            }
            continue;
        }
        if matches!(
            t,
            H264_NAL_AUD
                | H264_NAL_SPS
                | H264_NAL_PPS
                | H264_NAL_SEI
                | H264_NAL_PREFIX
                | H264_NAL_SUBSET_SPS
                | 16
                | 17
                | 18
        ) {
            s.samples.extend(cur.take());
        }
        match t {
            H264_NAL_AUD | H264_NAL_END_SEQ | H264_NAL_END_STREAM | H264_NAL_FILLER => {}
            H264_NAL_SPS | H264_NAL_PPS => pending.push(nal),
            _ => match &mut cur {
                Some(sample) => sample.nals.push(nal),
                None => pending.push(nal),
            },
        }
    }
    s.samples.extend(cur.take());
    if s.sps.is_empty() || s.pps.is_empty() {
        return Err(malformed("H.264 stream has no SPS or PPS"));
    }
    if !s.in_band {
        s.strip_param_sets(|nal| matches!(nal[0] & 0x1f, H264_NAL_SPS | H264_NAL_PPS));
    }
    Ok(s)
}
fn analyze_hevc<'a>(nals: &[&'a [u8]]) -> Result<MuxStream<'a>, Error> {
    let mut s = MuxStream {
        hevc_codec: true,
        ..MuxStream::default()
    };
    let mut spss: HashMap<u32, Rc<HevcSps>> = HashMap::new();
    let mut ppss = HashMap::new();
    let mut poc = HevcPoc::default();
    let mut cur: Option<MuxSample<'a>> = None;
    let mut pending: Vec<&'a [u8]> = Vec::new();
    let mut cvs = -1i64;
    let (mut started, mut skip_rasl) = (false, false);
    let mut dropping = false;
    for &nal in nals {
        if nal.len() < 3 || nal[0] & 0x80 != 0 {
            return Err(malformed("HEVC NAL unit header"));
        }
        let t = hevc_type(nal);
        match t {
            HEVC_NAL_VPS => {
                if !add_param_set(&mut s.vps, u32::from(nal[2] >> 4), nal) {
                    s.in_band = true;
                }
            }
            HEVC_NAL_SPS => {
                let sps = Rc::new(parse_hevc_sps(nal)?);
                if !add_param_set(&mut s.sps, sps.id, nal) {
                    s.in_band = true;
                }
                if s.hevc.is_none() {
                    (s.width, s.height) = (sps.width, sps.height);
                    s.hevc = Some(Rc::clone(&sps));
                }
                spss.insert(sps.id, sps);
            }
            HEVC_NAL_PPS => {
                let pps = parse_hevc_pps(nal)?;
                if !add_param_set(&mut s.pps, pps.id, nal) {
                    s.in_band = true;
                }
                ppss.insert(pps.id, pps);
            }
            HEVC_NAL_EOS => poc.after_eos = true,
            _ => {}
        }
        if t < 32 {
            let first = nal[2] & 0x80 != 0;
            if first || (cur.is_none() && !dropping) {
                s.samples.extend(cur.take());
                dropping = false;
                if !first {
                    return Err(malformed("HEVC picture without a first slice segment"));
                }
                let sl = parse_hevc_slice(nal, &spss, &ppss)?;
                if hevc_irap(t) {
                    started = true;
                }
                if !started || (skip_rasl && hevc_rasl(t)) {
                    s.dropped += 1;
                    dropping = true;
                    pending.clear();
                    continue;
                }
                let (order, new_cvs) = poc.next(&sl);
                if hevc_irap(t) {
                    skip_rasl = new_cvs && !hevc_idr(t);
                }
                if new_cvs {
                    cvs += 1;
                }
                cur = Some(MuxSample {
                    nals: std::mem::take(&mut pending),
                    sync: hevc_irap(t),
                    poc: order,
                    cvs,
                    pres: 0,
                });
            }
            if dropping {
                continue;
            }
            if let Some(sample) = &mut cur {
                sample.nals.push(nal);
            }
            continue;
        }
        if (HEVC_NAL_VPS..=HEVC_NAL_AUD).contains(&t)
            || t == HEVC_NAL_SEI_PREFIX
            || (41..=44).contains(&t)
            || (48..=55).contains(&t)
        {
            s.samples.extend(cur.take());
            dropping = false;
        }
        match t {
            HEVC_NAL_AUD | HEVC_NAL_EOS | HEVC_NAL_EOB | HEVC_NAL_FD => {}
            HEVC_NAL_VPS | HEVC_NAL_SPS | HEVC_NAL_PPS => pending.push(nal),
            _ => match &mut cur {
                Some(sample) => sample.nals.push(nal),
                None if !dropping => pending.push(nal),
                None => {}
            },
        }
    }
    s.samples.extend(cur.take());
    if s.vps.is_empty() || s.sps.is_empty() || s.pps.is_empty() {
        return Err(malformed("HEVC stream has no VPS, SPS or PPS"));
    }
    if !s.in_band {
        s.strip_param_sets(|nal| (HEVC_NAL_VPS..=HEVC_NAL_PPS).contains(&hevc_type(nal)));
    }
    Ok(s)
}
const UNITY_MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];
const SAMPLE_FLAGS_SYNC: u32 = 0x0200_0000;
const SAMPLE_FLAGS_NON_SYNC: u32 = 0x0101_0000;
impl<'a> MuxStream<'a> {
    fn order_presentation(&mut self) -> bool {
        let mut reordered = false;
        let mut i = 0;
        while i < self.samples.len() {
            let mut j = i + 1;
            while j < self.samples.len() && self.samples[j].cvs == self.samples[i].cvs {
                j += 1;
            }
            let mut idx: Vec<usize> = (i..j).collect();
            idx.sort_by_key(|&k| self.samples[k].poc);
            for (rank, &k) in idx.iter().enumerate() {
                self.samples[k].pres = i + rank;
                if k != i + rank {
                    reordered = true;
                }
            }
            i = j;
        }
        reordered
    }
    fn check_config(&self) -> Result<(), Error> {
        if self.width > 65535 || self.height > 65535 {
            return Err(unsupported_stream(format!(
                "picture size {}x{}",
                self.width, self.height
            )));
        }
        if !self.hevc_codec && (self.sps.len() > 31 || self.pps.len() > 255) {
            return Err(unsupported_stream(format!(
                "{} SPS and {} PPS exceed the avcC record",
                self.sps.len(),
                self.pps.len()
            )));
        }
        for list in [&self.vps, &self.sps, &self.pps] {
            if let Some(p) = list.iter().find(|p| p.raw.len() > 65535) {
                return Err(unsupported_stream(format!(
                    "parameter set of {} bytes",
                    p.raw.len()
                )));
            }
        }
        Ok(())
    }
    fn strip_param_sets(&mut self, is_param_set: impl Fn(&[u8]) -> bool) {
        for sample in &mut self.samples {
            sample.nals.retain(|nal| !is_param_set(nal));
        }
    }
    fn sample_entry(&self) -> (&'static str, String) {
        if let Some(sps) = &self.h264 {
            let entry = if self.in_band { "avc3" } else { "avc1" };
            return (
                entry,
                format!(
                    "{entry}.{:02X}{:02X}{:02X}",
                    sps.profile, sps.compat, sps.level
                ),
            );
        }
        let entry = if self.in_band { "hev1" } else { "hvc1" };
        let ptl = self.hevc.as_ref().map_or([0; 12], |sps| sps.ptl);
        (entry, hevc_codec_string(entry, &ptl))
    }
    fn init_segment(&self, info: &MuxInfo) -> Result<Vec<u8>, Error> {
        let total = (self.samples.len() as u64).wrapping_mul(u64::from(info.sample_duration));
        let Some(movie_duration) = total
            .wrapping_mul(1000)
            .checked_div(u64::from(info.timescale))
        else {
            return Err(Error::bare(Kind::FrameRateRequired));
        };
        let mut w = BoxWriter::default();
        let ftyp = w.open("ftyp");
        w.raw(b"iso6");
        w.u32(0);
        w.raw(b"iso6iso5mp41");
        w.raw(info.sample_entry.as_bytes());
        w.close(ftyp);
        let moov = w.open("moov");
        {
            let mvhd = w.full("mvhd", 1, 0);
            w.u64(0);
            w.u64(0);
            w.u32(1000);
            w.u64(movie_duration);
            w.u32(0x0001_0000);
            w.u16(0x0100);
            w.zeros(10);
            for m in UNITY_MATRIX {
                w.u32(m);
            }
            w.zeros(24);
            w.u32(2);
            w.close(mvhd);
            let trak = w.open("trak");
            {
                let tkhd = w.full("tkhd", 1, 3);
                w.u64(0);
                w.u64(0);
                w.u32(1);
                w.u32(0);
                w.u64(movie_duration);
                w.zeros(8);
                w.u16(0);
                w.u16(0);
                w.u16(0);
                w.u16(0);
                for m in UNITY_MATRIX {
                    w.u32(m);
                }
                w.u32((self.width as u32) << 16);
                w.u32((self.height as u32) << 16);
                w.close(tkhd);
                let mdia = w.open("mdia");
                {
                    let mdhd = w.full("mdhd", 1, 0);
                    w.u64(0);
                    w.u64(0);
                    w.u32(info.timescale);
                    w.u64(total);
                    w.u16(0x55c4);
                    w.u16(0);
                    w.close(mdhd);
                    let hdlr = w.full("hdlr", 0, 0);
                    w.u32(0);
                    w.raw(b"vide");
                    w.zeros(12);
                    w.raw(b"VideoHandler\0");
                    w.close(hdlr);
                    let minf = w.open("minf");
                    {
                        let vmhd = w.full("vmhd", 0, 1);
                        w.zeros(8);
                        w.close(vmhd);
                        let dinf = w.open("dinf");
                        let dref = w.full("dref", 0, 0);
                        w.u32(1);
                        let url = w.full("url ", 0, 1);
                        w.close(url);
                        w.close(dref);
                        w.close(dinf);
                        let stbl = w.open("stbl");
                        {
                            let stsd = w.full("stsd", 0, 0);
                            w.u32(1);
                            self.write_sample_entry(&mut w, info.sample_entry);
                            w.close(stsd);
                            for (name, zeros) in
                                [("stts", 4), ("stsc", 4), ("stsz", 8), ("stco", 4)]
                            {
                                let table = w.full(name, 0, 0);
                                w.zeros(zeros);
                                w.close(table);
                            }
                        }
                        w.close(stbl);
                    }
                    w.close(minf);
                }
                w.close(mdia);
            }
            w.close(trak);
            let mvex = w.open("mvex");
            let mehd = w.full("mehd", 1, 0);
            w.u64(movie_duration);
            w.close(mehd);
            let trex = w.full("trex", 0, 0);
            w.u32(1);
            w.u32(1);
            w.zeros(12);
            w.close(trex);
            w.close(mvex);
        }
        w.close(moov);
        Ok(w.b)
    }
    fn write_sample_entry(&self, w: &mut BoxWriter, entry: &str) {
        let visual = w.open(entry);
        w.zeros(6);
        w.u16(1);
        w.zeros(16);
        w.u16(self.width as u16);
        w.u16(self.height as u16);
        w.u32(0x0048_0000);
        w.u32(0x0048_0000);
        w.u32(0);
        w.u16(1);
        w.zeros(32);
        w.u16(0x0018);
        w.u16(0xffff);
        if let Some(sps) = &self.hevc {
            let config = w.open("hvcC");
            self.write_hvcc(w, sps);
            w.close(config);
        } else if let Some(sps) = &self.h264 {
            let config = w.open("avcC");
            self.write_avcc(w, sps);
            w.close(config);
        }
        w.close(visual);
    }
    fn write_avcc(&self, w: &mut BoxWriter, sps: &H264Sps<'_>) {
        w.u8(1);
        w.u8(sps.profile);
        w.u8(sps.compat);
        w.u8(sps.level);
        w.u8(0xfc | 3);
        w.u8(0xe0 | self.sps.len() as u8);
        for p in &self.sps {
            w.u16(p.raw.len() as u16);
            w.raw(p.raw);
        }
        w.u8(self.pps.len() as u8);
        for p in &self.pps {
            w.u16(p.raw.len() as u16);
            w.raw(p.raw);
        }
        if h264_high_profile(sps.profile) {
            w.u8(0xfc | sps.chroma_format as u8);
            w.u8(0xf8 | sps.bit_depth_luma.wrapping_sub(8) as u8);
            w.u8(0xf8 | sps.bit_depth_chroma.wrapping_sub(8) as u8);
            w.u8(0);
        }
    }
    fn write_hvcc(&self, w: &mut BoxWriter, sps: &HevcSps) {
        w.u8(1);
        w.raw(&sps.ptl);
        w.u16(0xf000);
        w.u8(0xfc);
        w.u8(0xfc | sps.chroma_format as u8);
        w.u8(0xf8 | sps.bit_depth_luma.wrapping_sub(8) as u8);
        w.u8(0xf8 | sps.bit_depth_chroma.wrapping_sub(8) as u8);
        w.u16(0);
        let nesting = u8::from(sps.temporal_id_nesting);
        w.u8((sps.max_sub_layers as u8) << 3 | nesting << 2 | 3);
        w.u8(3);
        let complete = if self.in_band { 0 } else { 0x80 };
        for (t, sets) in [
            (HEVC_NAL_VPS, &self.vps),
            (HEVC_NAL_SPS, &self.sps),
            (HEVC_NAL_PPS, &self.pps),
        ] {
            w.u8(complete | t);
            w.u16(sets.len() as u16);
            for p in sets {
                w.u16(p.raw.len() as u16);
                w.raw(p.raw);
            }
        }
    }
    fn write_fragment(
        &self,
        dst: &mut CountingWriter<'_>,
        seq: u32,
        start: usize,
        end: usize,
        duration: u32,
    ) -> Result<(), Error> {
        let samples = &self.samples[start..end];
        let mut sizes = Vec::with_capacity(samples.len());
        let mut payload = 0u64;
        for sm in samples {
            let n: u64 = sm.nals.iter().map(|nal| 4 + nal.len() as u64).sum();
            if n == 0 || n > u64::from(u32::MAX) {
                return Err(malformed(format!("access unit size {n}")));
            }
            sizes.push(n as u32);
            payload += n;
        }
        if payload + 8 > u64::from(u32::MAX) {
            return Err(Error::new(Kind::MediaTooLarge, "fragment exceeds 4 GiB"));
        }
        let mut w = BoxWriter::default();
        let moof = w.open("moof");
        let mfhd = w.full("mfhd", 0, 0);
        w.u32(seq);
        w.close(mfhd);
        let traf = w.open("traf");
        let tfhd = w.full("tfhd", 0, 0x02_0000);
        w.u32(1);
        w.close(tfhd);
        let tfdt = w.full("tfdt", 1, 0);
        w.u64((start as u64).wrapping_mul(u64::from(duration)));
        w.close(tfdt);
        let trun = w.full("trun", 1, 0x00_0f01);
        w.u32(samples.len() as u32);
        let data_offset_pos = w.b.len();
        w.u32(0);
        for (i, sm) in samples.iter().enumerate() {
            w.u32(duration);
            w.u32(sizes[i]);
            w.u32(if sm.sync {
                SAMPLE_FLAGS_SYNC
            } else {
                SAMPLE_FLAGS_NON_SYNC
            });
            let shift = (sm.pres as i64 - (start + i) as i64) as i32;
            w.u32(shift.wrapping_mul(duration as i32) as u32);
        }
        w.close(trun);
        w.close(traf);
        w.close(moof);
        let data_offset = (w.b.len() + 8) as u32;
        w.b[data_offset_pos..data_offset_pos + 4].copy_from_slice(&data_offset.to_be_bytes());
        w.u32((payload + 8) as u32);
        w.raw(b"mdat");
        dst.write(&w.b)?;
        for sm in samples {
            for nal in &sm.nals {
                dst.write(&(nal.len() as u32).to_be_bytes())?;
                dst.write(nal)?;
            }
        }
        Ok(())
    }
}
#[derive(Default)]
struct BoxWriter {
    b: Vec<u8>,
}
impl BoxWriter {
    fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.b.extend_from_slice(&v.to_be_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_be_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.b.extend_from_slice(&v.to_be_bytes());
    }
    fn raw(&mut self, p: &[u8]) {
        self.b.extend_from_slice(p);
    }
    fn zeros(&mut self, n: usize) {
        self.b.resize(self.b.len() + n, 0);
    }
    fn open(&mut self, typ: &str) -> usize {
        let start = self.b.len();
        self.u32(0);
        self.raw(typ.as_bytes());
        start
    }
    fn full(&mut self, typ: &str, version: u8, flags: u32) -> usize {
        let start = self.open(typ);
        self.u32(u32::from(version) << 24 | flags & 0x00ff_ffff);
        start
    }
    fn close(&mut self, start: usize) {
        let size = (self.b.len() - start) as u32;
        self.b[start..start + 4].copy_from_slice(&size.to_be_bytes());
    }
}
struct CountingWriter<'a> {
    dst: &'a mut dyn Write,
    n: i64,
}
impl CountingWriter<'_> {
    fn write(&mut self, p: &[u8]) -> Result<(), Error> {
        match self.dst.write(p) {
            Ok(n) => {
                self.n += n.min(p.len()) as i64;
                if n < p.len() {
                    return Err(Error::other("short write"));
                }
                Ok(())
            }
            Err(err) => Err(Error::other(err.to_string())),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timing() {
        assert_eq!(media_timing(30000.0 / 1001.0), (30000, 1001));
        assert_eq!(media_timing(25.0), (90000, 3600));
        assert_eq!(media_timing(7.3), (7300, 1000));
    }
    #[test]
    fn boxes_nest() {
        let mut w = BoxWriter::default();
        let outer = w.open("moov");
        let inner = w.full("mehd", 1, 0);
        w.u64(7);
        w.close(inner);
        w.close(outer);
        assert_eq!(w.b.len(), 28);
        assert_eq!(&w.b[..8], b"\0\0\0\x1cmoov");
        assert_eq!(&w.b[8..16], b"\0\0\0\x14mehd");
    }
}
