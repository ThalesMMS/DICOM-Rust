pub const CAPABILITIES: u16 = 0x8100;
pub const MAX_FRAMES: i64 = 16;
pub const MAX_SAMPLES: i64 = 1 << 21;
pub(crate) const ERR_UNSUPPORTED_COLLECTION: &str = "jpeg2000: unsupported Annex J collection";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Profile {
    pub frames: i64,
    pub bits: i64,
    pub signed: bool,
    pub reversible: bool,
}
impl Profile {
    pub fn valid(&self) -> bool {
        self.frames >= 2
            && self.frames <= MAX_FRAMES
            && self.bits >= 1
            && ((self.reversible && self.bits <= 16) || (!self.reversible && self.bits == 8))
    }
    pub fn shift(&self) -> i32 {
        if self.signed {
            0
        } else {
            1i32.wrapping_shl(self.bits.wrapping_sub(1) as u32)
        }
    }
    pub(crate) fn coded_bits(&self) -> i64 {
        self.bits.saturating_add(1)
    }
    pub fn markers(&self) -> Vec<u8> {
        let mut depth = self.bits.wrapping_sub(1) as u8;
        if self.signed {
            depth |= 0x80;
        }
        let mut cbd = (self.frames as u16).to_be_bytes().to_vec();
        for _ in 0..self.frames {
            cbd.push(depth);
        }
        let mut out = append_marker(Vec::new(), 0xff78, &cbd);
        const DEPENDENCY32: u16 = 0x0401;
        const DECORRELATION32: u16 = 0x0901;
        const OFFSET32: u16 = 0x0602;
        let kind = if self.reversible {
            DEPENDENCY32
        } else {
            DECORRELATION32
        };
        let mut matrix = vec![0, 0, (kind >> 8) as u8, 1, 0, 0];
        if self.reversible {
            for row in 1..self.frames {
                for column in 0..=row {
                    let coefficient: u32 = if column >= row.saturating_sub(1) {
                        1
                    } else {
                        0
                    };
                    matrix.extend_from_slice(&coefficient.to_be_bytes());
                }
            }
        } else {
            for row in 0..self.frames {
                for column in 0..self.frames {
                    let coefficient: u32 = if column <= row { 1f32.to_bits() } else { 0 };
                    matrix.extend_from_slice(&coefficient.to_be_bytes());
                }
            }
        }
        out = append_marker(out, 0xff74, &matrix);
        let mut offsets = vec![0, 0, (OFFSET32 >> 8) as u8, 2, 0, 0];
        for frame in 0..self.frames {
            let mut offset = self.shift();
            if self.reversible && frame != 0 {
                offset = 0;
            }
            offsets.extend_from_slice(&(offset as u32).to_be_bytes());
        }
        out = append_marker(out, 0xff74, &offsets);
        let (kind, reversible) = if self.reversible { (0u8, 1u8) } else { (1, 0) };
        let mut mcc = vec![0, 0, 1, 0, 0, 0, 1, kind];
        for _ in 0..2 {
            mcc.extend_from_slice(&(self.frames as u16).to_be_bytes());
            for i in 0..self.frames {
                mcc.push(i as u8);
            }
        }
        mcc.extend_from_slice(&[reversible, 2, 1]);
        out = append_marker(out, 0xff75, &mcc);
        append_marker(out, 0xff77, &[1, 1])
    }
}
fn append_marker(mut dst: Vec<u8>, marker: u16, body: &[u8]) -> Vec<u8> {
    dst.extend_from_slice(&marker.to_be_bytes());
    dst.extend_from_slice(&(body.len().saturating_add(2) as u16).to_be_bytes());
    dst.extend_from_slice(body);
    dst
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Collection {
    pub profile: Profile,
    pub rows: i64,
    pub columns: i64,
    marker_start: usize,
    marker_end: usize,
    cod_mct: usize,
    end: usize,
}
fn be16(b: &[u8]) -> u16 {
    b.first_chunk::<2>().map_or(0, |&x| u16::from_be_bytes(x))
}
fn be32(b: &[u8]) -> u32 {
    b.first_chunk::<4>().map_or(0, |&x| u32::from_be_bytes(x))
}
fn tail(b: &[u8], at: usize) -> &[u8] {
    b.get(at..).unwrap_or(&[])
}
pub fn inspect(data: &[u8]) -> Result<Collection, &'static str> {
    const ERR: &str = ERR_UNSUPPORTED_COLLECTION;
    if data.len() < 2 || be16(data) != 0xff4f {
        return Err(ERR);
    }
    let mut position = 2usize;
    let read = |position: &mut usize, marker: u16| -> Option<(usize, usize)> {
        let rest = data.get(*position..)?;
        if rest.len() < 4 || be16(rest) != marker {
            return None;
        }
        let length = be16(tail(rest, 2)) as usize;
        if length < 2 || length > rest.len().saturating_sub(2) {
            return None;
        }
        let start = position.saturating_add(4);
        *position = position.saturating_add(2).saturating_add(length);
        Some((start, *position))
    };
    let segment = |at: Option<(usize, usize)>| at.and_then(|(a, b)| data.get(a..b)).unwrap_or(&[]);
    let siz = segment(read(&mut position, 0xff51));
    if siz.len() < 36 || be16(siz) != CAPABILITIES {
        return Err(ERR);
    }
    let (width, height) = (be32(tail(siz, 2)), be32(tail(siz, 6)));
    let frames = be16(tail(siz, 34)) as i64;
    if width == 0
        || height == 0
        || width > 65535
        || height > 65535
        || !(2..=MAX_FRAMES).contains(&frames)
        || siz.len() as i64 != frames.saturating_mul(3).saturating_add(36)
        || (width as u64)
            .saturating_mul(height as u64)
            .saturating_mul(frames as u64)
            > MAX_SAMPLES as u64
    {
        return Err(ERR);
    }
    if siz.get(10..18) != Some(&[0; 8][..])
        || siz.get(26..34) != Some(&[0; 8][..])
        || be32(tail(siz, 18)) != width
        || be32(tail(siz, 22)) != height
    {
        return Err(ERR);
    }
    let cod_mct = position.saturating_add(8);
    let cod = segment(read(&mut position, 0xff52));
    let &[0, 0, 0, 1, 2, levels, 4, 4, 0, wavelet] = cod else {
        return Err(ERR);
    };
    if levels > 16 || wavelet > 1 {
        return Err(ERR);
    }
    let reversible = wavelet == 1;
    let qcd = segment(read(&mut position, 0xff5c));
    let band_count = usize::from(levels).saturating_mul(3).saturating_add(1);
    let sqcd = qcd.first().map_or(0xFF, |&q| q & 31);
    if reversible {
        if qcd.len() != band_count.saturating_add(1) || sqcd != 0 {
            return Err(ERR);
        }
    } else if qcd.len() != band_count.saturating_mul(2).saturating_add(1) || sqcd != 2 {
        return Err(ERR);
    }
    let marker_start = position;
    let cbd = segment(read(&mut position, 0xff78));
    if cbd.len() as i64 != frames.saturating_add(2) || be16(cbd) != frames as u16 {
        return Err(ERR);
    }
    let Some(&depth) = cbd.get(2) else {
        return Err(ERR);
    };
    let profile = Profile {
        frames,
        bits: i64::from(depth & 0x7f).saturating_add(1),
        signed: depth & 0x80 != 0,
        reversible,
    };
    if !profile.valid() {
        return Err(ERR);
    }
    let coded = (profile.coded_bits().saturating_sub(1) as u8) | 0x80;
    for &entry in tail(siz, 36).as_chunks::<3>().0 {
        if entry != [coded, 1, 1] {
            return Err(ERR);
        }
    }
    let markers = profile.markers();
    if !tail(data, marker_start).starts_with(&markers) {
        return Err(ERR);
    }
    position = marker_start.saturating_add(markers.len());
    let marker_end = position;
    let sot = segment(read(&mut position, 0xff90));
    let &[i0, i1, p0, p1, p2, p3, 0, 1] = sot else {
        return Err(ERR);
    };
    let after = tail(data, position);
    if u16::from_be_bytes([i0, i1]) != 0 || after.len() < 2 || be16(after) != 0xff93 {
        return Err(ERR);
    }
    let tile_bytes = u32::from_be_bytes([p0, p1, p2, p3]) as i64;
    if tile_bytes < 14
        || tile_bytes > data.len().saturating_sub(marker_end).saturating_sub(2) as i64
    {
        return Err(ERR);
    }
    let end = marker_end
        .saturating_add(tile_bytes as usize)
        .saturating_add(2);
    if be16(tail(data, end.saturating_sub(2))) != 0xffd9
        || (end != data.len()
            && (end % 2 != 1 || data.len().saturating_sub(end) != 1 || data.get(end) != Some(&0)))
    {
        return Err(ERR);
    }
    Ok(Collection {
        profile,
        rows: height as i64,
        columns: width as i64,
        marker_start,
        marker_end,
        cod_mct,
        end,
    })
}
impl Collection {
    pub(crate) fn coded_components(&self, data: &[u8]) -> Vec<u8> {
        let removed = self.marker_end.saturating_sub(self.marker_start);
        let mut out = Vec::with_capacity(self.end.saturating_sub(removed));
        out.extend_from_slice(data.get(..self.marker_start).unwrap_or(&[]));
        for at in [6, 7, self.cod_mct] {
            if let Some(b) = out.get_mut(at) {
                *b = 0;
            }
        }
        out.extend_from_slice(data.get(self.marker_end..self.end).unwrap_or(&[]));
        out
    }
}
