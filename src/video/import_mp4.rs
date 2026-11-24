use super::avc::{AvcProfile, normalize_avc_rate};
use super::h264::{
    H264_NAL_IDR, H264_NAL_PPS, H264_NAL_SLICE, H264_NAL_SPS, H264Sps, parse_h264_pps,
    parse_h264_sps,
};
use super::import::{ImportLimits, SourceInfo};
use super::nal::{BitReader, rbsp};
use super::{
    CONTAINER_MP4, DEFAULT_MAX_FRAGMENTS, Error, Kind, import_error, import_media, too_large,
};
use crate::object::transfer;
const MPEG4_HP41: &str = "1.2.840.10008.1.2.4.102";
#[derive(Clone, Copy)]
struct ImportBox {
    kind: [u8; 4],
    start: usize,
    payload: usize,
    end: usize,
}
struct Mp4Import<'a> {
    data: &'a [u8],
    boxes: usize,
    nals: usize,
}
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
fn be64(b: &[u8]) -> u64 {
    u64::from(be32(b)) << 32 | u64::from(be32(&b[4..]))
}
fn be16(b: &[u8]) -> usize {
    usize::from(b[0]) << 8 | usize::from(b[1])
}
fn one_box(boxes: &[ImportBox], kind: &str) -> Result<ImportBox, Error> {
    let mut found = boxes.iter().filter(|b| b.kind == kind.as_bytes());
    match (found.next(), found.next()) {
        (Some(b), None) => Ok(*b),
        _ => Err(import_error(&format!("missing or duplicate {kind}"))),
    }
}
fn has_box(boxes: &[ImportBox], kind: &str) -> bool {
    boxes.iter().any(|b| b.kind == kind.as_bytes())
}
impl<'a> Mp4Import<'a> {
    fn boxes_at(&mut self, mut start: usize, end: usize) -> Result<Vec<ImportBox>, Error> {
        let mut boxes = Vec::new();
        while start < end {
            if self.boxes >= 4096 {
                return Err(too_large());
            }
            self.boxes += 1;
            if end - start < 8 {
                return Err(import_error("truncated box"));
            }
            let mut size = u64::from(be32(&self.data[start..]));
            let mut header = 8usize;
            if size == 1 {
                if end - start < 16 {
                    return Err(import_media());
                }
                size = be64(&self.data[start + 8..]);
                header = 16;
            }
            if size == 0 {
                size = (end - start) as u64;
            }
            if size < header as u64 || size > (end - start) as u64 {
                return Err(import_media());
            }
            let size = size as usize;
            boxes.push(ImportBox {
                kind: [
                    self.data[start + 4],
                    self.data[start + 5],
                    self.data[start + 6],
                    self.data[start + 7],
                ],
                start,
                payload: start + header,
                end: start + size,
            });
            start += size;
        }
        Ok(boxes)
    }
    fn children(&mut self, b: ImportBox) -> Result<Vec<ImportBox>, Error> {
        self.boxes_at(b.payload, b.end)
    }
    fn body(&self, b: ImportBox) -> &'a [u8] {
        &self.data[b.payload..b.end]
    }
}
pub(crate) fn inspect_mp4(data: &[u8], limits: ImportLimits) -> Result<SourceInfo, Error> {
    inspect_mp4_profile(data, limits, None)
}
pub(crate) fn inspect_mp4_profile(
    data: &[u8],
    limits: ImportLimits,
    mut profile: Option<&mut AvcProfile>,
) -> Result<SourceInfo, Error> {
    let mut p = Mp4Import {
        data,
        boxes: 0,
        nals: 0,
    };
    let top = p.boxes_at(0, data.len())?;
    if has_box(&top, "moof") {
        return Err(import_error("fragmented MP4"));
    }
    let ftyp = one_box(&top, "ftyp")?;
    let brands = p.body(ftyp);
    if ftyp.start != 0 || brands.len() < 8 || brands.len() & 3 != 0 {
        return Err(import_media());
    }
    let known = brands
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .any(|(i, brand)| {
            i != 1 && matches!(brand, b"isom" | b"iso2" | b"mp41" | b"mp42" | b"avc1")
        });
    if !known {
        return Err(import_error("unsupported MP4 brand"));
    }
    let moov = one_box(&top, "moov")?;
    let movie = p.children(moov)?;
    let trak = one_box(&movie, "trak")?;
    let tracks = p.children(trak)?;
    if has_box(&tracks, "edts") {
        return Err(import_error("edited MP4"));
    }
    let tk = p.body(one_box(&tracks, "tkhd")?);
    let mut matrix = 40usize;
    if tk.len() < 84 {
        return Err(import_media());
    }
    if tk[0] == 1 {
        matrix = 52;
        if tk.len() < 96 {
            return Err(import_media());
        }
    } else if tk[0] != 0 {
        return Err(import_media());
    }
    const IDENTITY: [u32; 9] = [65536, 0, 0, 0, 65536, 0, 0, 0, 1 << 30];
    for (i, want) in IDENTITY.into_iter().enumerate() {
        if be32(&tk[matrix + i * 4..]) != want {
            return Err(import_error("rotated or scaled track"));
        }
    }
    let (width, height) = (be32(&tk[matrix + 36..]), be32(&tk[matrix + 40..]));
    if width & 65535 != 0 || height & 65535 != 0 {
        return Err(import_media());
    }
    let mdia = one_box(&tracks, "mdia")?;
    let media = p.children(mdia)?;
    let h = p.body(one_box(&media, "hdlr")?);
    if h.len() < 12 || &h[8..12] != b"vide" {
        return Err(import_error("non-video track"));
    }
    let md = p.body(one_box(&media, "mdhd")?);
    if md.len() < 20 {
        return Err(import_media());
    }
    let (timescale, duration) = if md[0] == 0 {
        (be32(&md[12..]), u64::from(be32(&md[16..])))
    } else if md[0] == 1 && md.len() >= 32 {
        (be32(&md[20..]), be64(&md[24..]))
    } else {
        return Err(import_media());
    };
    if timescale == 0 {
        return Err(import_media());
    }
    let minf = one_box(&media, "minf")?;
    let minf_children = p.children(minf)?;
    let stbl = one_box(&minf_children, "stbl")?;
    let tables = p.children(stbl)?;
    let stsd = one_box(&tables, "stsd")?;
    let sd = p.body(stsd);
    if sd.len() < 8 || be32(&sd[4..]) != 1 {
        return Err(import_media());
    }
    let entry = match p.boxes_at(stsd.payload + 8, stsd.end).as_deref() {
        Ok(&[entry]) if &entry.kind == b"avc1" => entry,
        _ => return Err(import_error("unsupported MP4 sample entry")),
    };
    let visual = p.body(entry);
    if visual.len() < 78 {
        return Err(import_media());
    }
    let (columns, rows) = (be16(&visual[24..]) as i64, be16(&visual[26..]) as i64);
    if columns != i64::from(width >> 16) || rows != i64::from(height >> 16) {
        return Err(import_media());
    }
    let config = p.boxes_at(entry.payload + 78, entry.end)?;
    let avcc = one_box(&config, "avcC")?;
    if has_box(&config, "pasp") {
        let a = p.body(one_box(&config, "pasp")?);
        if a.len() != 8 || be32(a) != 1 || be32(&a[4..]) != 1 {
            return Err(import_error("non-square MP4 pixel aspect"));
        }
    }
    let length_size = inspect_avcc(p.body(avcc), columns, rows, profile.as_deref_mut())?;
    if let Some(profile) = profile.as_deref_mut() {
        inspect_avc_colour(&p, &config, profile)?;
    }
    let sz = p.body(one_box(&tables, "stsz")?);
    if sz.len() < 12 {
        return Err(import_media());
    }
    let (constant, n) = (be32(&sz[4..]), be32(&sz[8..]));
    if n == 0 || u64::from(n) > limits.max_frames as u64 {
        return Err(too_large());
    }
    if rows < 1
        || columns < 1
        || rows * columns > limits.max_pixels
        || rows * columns > limits.max_decoded_bytes / 3 / i64::from(n)
    {
        return Err(too_large());
    }
    if (constant == 0 && sz.len() as u64 != 12 + u64::from(n) * 4)
        || (constant != 0 && sz.len() != 12)
    {
        return Err(import_media());
    }
    let mut sizes = vec![constant; n as usize];
    for (i, size) in sizes.iter_mut().enumerate() {
        if constant == 0 {
            *size = be32(&sz[12 + i * 4..]);
        }
        if *size == 0 {
            return Err(import_media());
        }
    }
    let ts = p.body(one_box(&tables, "stts")?);
    if ts.len() < 8 {
        return Err(import_media());
    }
    let count = u64::from(be32(&ts[4..]));
    if count == 0 || count > limits.max_frames as u64 || ts.len() as u64 != 8 + count * 8 {
        return Err(import_media());
    }
    let mut frames = 0u64;
    let mut delta = 0u32;
    for i in 0..count as usize {
        let samples = be32(&ts[8 + i * 8..]);
        let d = be32(&ts[12 + i * 8..]);
        if samples == 0 || d == 0 || (delta != 0 && delta != d) {
            return Err(import_error("nonconstant video timing"));
        }
        delta = d;
        frames += u64::from(samples);
        if frames > limits.max_frames as u64 {
            return Err(too_large());
        }
    }
    let macroblocks = ((columns + 15) / 16) as u64 * ((rows + 15) / 16) as u64;
    if columns > 4096
        || rows > 4096
        || macroblocks > 8192
        || macroblocks as f64 * f64::from(timescale) / f64::from(delta) > 245_760.0
    {
        return Err(import_error("AVC exceeds Level4.1 geometry/rate"));
    }
    if frames != u64::from(n) || duration != frames * u64::from(delta) {
        return Err(import_error("inconsistent frame timing"));
    }
    if let Some(profile) = profile {
        inspect_avc_presentation(&p, &tables, n, delta)?;
        let (numerator, denominator) = normalize_avc_rate(timescale, delta);
        (profile.rows, profile.columns, profile.frames) = (rows, columns, i64::from(n));
        (profile.rate_numerator, profile.rate_denominator) = (numerator, denominator);
        profile.validate()?;
    }
    p.inspect_samples(&tables, &top, &sizes, length_size)?;
    Ok(SourceInfo {
        rows,
        columns,
        frames: i64::from(n),
        frame_rate: f64::from(timescale) / f64::from(delta),
        transfer_syntax: transfer::lookup(MPEG4_HP41).ok_or_else(import_media)?,
        container: CONTAINER_MP4,
        compression_method: "ISO_14496_10",
    })
}
fn inspect_avcc(
    b: &[u8],
    columns: i64,
    rows: i64,
    mut profile: Option<&mut AvcProfile>,
) -> Result<usize, Error> {
    if b.len() < 7
        || b[0] != 1
        || b[1] != 100
        || !admitted_avc_level(b[3])
        || b[4] & 0xfc != 0xfc
        || b[5] & 0xe0 != 0xe0
    {
        return Err(import_error("AVC profile/level"));
    }
    let length_size = usize::from(b[4] & 3) + 1;
    if length_size != 4 {
        return Err(import_error("AVC NAL length width"));
    }
    let mut n = usize::from(b[5] & 31);
    if n == 0 {
        return Err(import_media());
    }
    let mut at = 6usize;
    let mut sps_ids = Vec::new();
    for i in 0..n {
        if at + 2 > b.len() {
            return Err(import_media());
        }
        let size = be16(&b[at..]);
        at += 2;
        if size < 4 || size > b.len() - at || b[at] & 31 != H264_NAL_SPS {
            return Err(import_media());
        }
        let s = parse_h264_sps(&b[at..at + size])?;
        if b[1] != b[at + 1] || b[2] != b[at + 2] || b[3] != b[at + 3] {
            return Err(import_error("AVC configuration/header mismatch"));
        }
        if s.profile != 100
            || !admitted_avc_level(s.level)
            || s.chroma_format != 1
            || s.separate_colour_plane
            || s.bit_depth_luma != 8
            || s.bit_depth_chroma != 8
            || !s.frame_mbs_only
            || s.width != columns
            || s.height != rows
        {
            return Err(import_error("AVC sample geometry/profile"));
        }
        inspect_import_avc_aspect(&s)?;
        if let Some(profile) = profile.as_deref_mut() {
            let colour = avc_limited_colour(&s)?;
            if i > 0 && colour != profile.colour {
                return Err(import_error("changing AVC colour signal"));
            }
            profile.colour = colour;
        }
        if sps_ids.contains(&s.id) {
            return Err(import_media());
        }
        sps_ids.push(s.id);
        at += size;
    }
    if at >= b.len() {
        return Err(import_media());
    }
    n = usize::from(b[at]);
    at += 1;
    if n == 0 {
        return Err(import_media());
    }
    for _ in 0..n {
        if at + 2 > b.len() {
            return Err(import_media());
        }
        let size = be16(&b[at..]);
        at += 2;
        if size < 2 || size > b.len() - at || b[at] & 31 != H264_NAL_PPS {
            return Err(import_media());
        }
        let pps = parse_h264_pps(&b[at..at + size])?;
        if !sps_ids.contains(&pps.sps_id) {
            return Err(import_media());
        }
        at += size;
    }
    if at < b.len()
        && (b.len() - at != 4
            || b[at] & 3 != 1
            || b[at + 1] & 7 != 0
            || b[at + 2] & 7 != 0
            || b[at + 3] != 0)
    {
        return Err(import_error("AVC configuration extension"));
    }
    Ok(length_size)
}
impl Mp4Import<'_> {
    fn inspect_samples(
        &mut self,
        tables: &[ImportBox],
        top: &[ImportBox],
        sizes: &[u32],
        length_size: usize,
    ) -> Result<(), Error> {
        let sc = self.body(one_box(tables, "stsc")?);
        if sc.len() < 8 {
            return Err(import_media());
        }
        let n = u64::from(be32(&sc[4..]));
        if n == 0 || n > sizes.len() as u64 || sc.len() as u64 != 8 + n * 12 {
            return Err(import_media());
        }
        if has_box(tables, "stco") && has_box(tables, "co64") {
            return Err(import_media());
        }
        let (kind, width) = if has_box(tables, "co64") {
            ("co64", 8usize)
        } else {
            ("stco", 4usize)
        };
        let co = self.body(one_box(tables, kind)?);
        if co.len() < 8 {
            return Err(import_media());
        }
        let chunks = u64::from(be32(&co[4..]));
        if chunks == 0
            || chunks > sizes.len() as u64
            || co.len() as u64 != 8 + chunks * width as u64
        {
            return Err(import_media());
        }
        let offsets: Vec<u64> = (0..chunks as usize)
            .map(|i| {
                if width == 4 {
                    u64::from(be32(&co[8 + i * 4..]))
                } else {
                    be64(&co[8 + i * 8..])
                }
            })
            .collect();
        let n = n as usize;
        let mut firsts = Vec::with_capacity(n);
        let mut per = Vec::with_capacity(n);
        for i in 0..n {
            let first = be32(&sc[8 + i * 12..]);
            let samples = be32(&sc[12 + i * 12..]);
            let desc = be32(&sc[16 + i * 12..]);
            if first < 1
                || u64::from(first) > chunks
                || (i == 0 && first != 1)
                || (i > 0 && first <= firsts[i - 1])
                || samples == 0
                || desc != 1
            {
                return Err(import_media());
            }
            firsts.push(first);
            per.push(samples);
        }
        let data_len = self.data.len() as u64;
        let (mut index, mut entry) = (0usize, 0usize);
        let mut previous_end = 0u64;
        for (c, &chunk_offset) in offsets.iter().enumerate() {
            let mut offset = chunk_offset;
            while entry + 1 < firsts.len() && firsts[entry + 1] <= (c + 1) as u32 {
                entry += 1;
            }
            if offset < previous_end {
                return Err(import_error("overlapping video samples"));
            }
            for _ in 0..per[entry] {
                if index >= sizes.len()
                    || offset > data_len
                    || u64::from(sizes[index]) > data_len - offset
                {
                    return Err(import_media());
                }
                let end = offset + u64::from(sizes[index]);
                let in_media = top.iter().any(|m| {
                    &m.kind == b"mdat" && offset >= m.payload as u64 && end <= m.end as u64
                });
                if !in_media {
                    return Err(import_error("sample outside mdat"));
                }
                self.inspect_avc_sample(&self.data[offset as usize..end as usize], length_size)?;
                offset = end;
                index += 1;
            }
            previous_end = offset;
        }
        if index != sizes.len() {
            return Err(import_media());
        }
        Ok(())
    }
    fn inspect_avc_sample(&mut self, b: &[u8], width: usize) -> Result<(), Error> {
        let mut coded = false;
        let mut at = 0usize;
        while at < b.len() {
            self.nals += 1;
            if self.nals > DEFAULT_MAX_FRAGMENTS {
                return Err(Error::bare(Kind::TooManyFragments));
            }
            if b.len() - at < width {
                return Err(import_media());
            }
            let n = u64::from(be32(&b[at..]));
            at += width;
            if n == 0 || n > (b.len() - at) as u64 || b[at] & 128 != 0 {
                return Err(import_media());
            }
            let kind = b[at] & 31;
            if kind == H264_NAL_SPS || kind == H264_NAL_PPS {
                return Err(import_error("in-band AVC configuration change"));
            }
            if kind == H264_NAL_SLICE || kind == H264_NAL_IDR {
                coded = true;
            }
            at += n as usize;
        }
        if !coded {
            return Err(import_error("sample without coded picture"));
        }
        Ok(())
    }
}
fn avc_limited_colour(s: &H264Sps<'_>) -> Result<[u8; 3], Error> {
    let mut colour = [2u8; 3];
    let data = rbsp(&s.raw[1..]);
    let mut r = BitReader::new(&data);
    r.pos = s.vui_bit_offset;
    if r.flag() {
        if r.flag() {
            r.u(8);
        }
        if r.flag() {
            r.flag();
        }
        if r.flag() {
            r.u(3);
            if r.flag() {
                return Err(import_error("full-range AVC samples"));
            }
            if r.flag() {
                for c in &mut colour {
                    *c = r.u(8) as u8;
                }
            }
        }
        if r.flag() {
            let (top, bottom) = (r.ue(), r.ue());
            if top != 0 || bottom != 0 {
                return Err(import_error("nondefault AVC chroma location"));
            }
        }
    }
    match r.err {
        Some(err) => Err(err),
        None => Ok(colour),
    }
}
fn inspect_avc_colour(
    p: &Mp4Import<'_>,
    config: &[ImportBox],
    profile: &mut AvcProfile,
) -> Result<(), Error> {
    if !has_box(config, "colr") {
        return Ok(());
    }
    let body = p.body(one_box(config, "colr")?);
    if body.len() != 10 && body.len() != 11 {
        return Err(import_error("unsupported MP4 colour signal"));
    }
    match &body[..4] {
        b"nclc" => {
            if body.len() != 10 {
                return Err(import_media());
            }
        }
        b"nclx" => {
            if body.len() != 11 || body[10] != 0 {
                return Err(import_error("full-range MP4 colour"));
            }
        }
        _ => return Err(import_error("unsupported MP4 colour signal")),
    }
    for (i, c) in profile.colour.iter_mut().enumerate() {
        let value = be16(&body[4 + 2 * i..]);
        if value > 255 || (*c != 2 && usize::from(*c) != value) {
            return Err(import_error("conflicting AVC and MP4 colour signals"));
        }
        *c = value as u8;
    }
    Ok(())
}
fn inspect_avc_presentation(
    p: &Mp4Import<'_>,
    tables: &[ImportBox],
    frames: u32,
    delta: u32,
) -> Result<(), Error> {
    if !has_box(tables, "ctts") {
        return Ok(());
    }
    let table = p.body(one_box(tables, "ctts")?);
    if table.len() < 8 || (table[0] != 0 && table[0] != 1) {
        return Err(import_media());
    }
    let count = be32(&table[4..]);
    if count == 0 || count > frames || table.len() as u64 != 8 + 8 * u64::from(count) {
        return Err(import_media());
    }
    let mut presentation: Vec<i64> = Vec::with_capacity(frames as usize);
    for i in 0..count as usize {
        let at = 8 + 8 * i;
        let samples = be32(&table[at..]);
        let mut offset = i64::from(be32(&table[at + 4..]));
        if table[0] == 1 {
            offset = i64::from(offset as u32 as i32);
        }
        if samples == 0 || samples > frames - presentation.len() as u32 {
            return Err(import_media());
        }
        for _ in 0..samples {
            presentation.push(presentation.len() as i64 * i64::from(delta) + offset);
        }
    }
    if presentation.len() as u32 != frames {
        return Err(import_media());
    }
    presentation.sort_unstable();
    for (i, pts) in presentation.iter().enumerate() {
        if pts - presentation[0] != i as i64 * i64::from(delta) {
            return Err(import_error("nonconstant presentation timing"));
        }
    }
    Ok(())
}
fn admitted_avc_level(level: u8) -> bool {
    matches!(
        level,
        10 | 11 | 12 | 13 | 20 | 21 | 22 | 30 | 31 | 32 | 40 | 41
    )
}
fn inspect_import_avc_aspect(s: &H264Sps<'_>) -> Result<(), Error> {
    let data = rbsp(&s.raw[1..]);
    let mut r = BitReader::new(&data);
    r.pos = s.vui_bit_offset;
    if r.flag() && r.flag() && r.u(8) != 1 {
        return Err(import_error("AVC non-square sample aspect"));
    }
    if let Some(err) = r.err.take() {
        return Err(err);
    }
    let remaining_one =
        (r.pos..data.len() * 8).any(|pos| data[pos / 8] & (1 << (7 - pos % 8)) != 0);
    if !remaining_one {
        return Err(import_error("truncated AVC VUI/trailing bits"));
    }
    Ok(())
}
