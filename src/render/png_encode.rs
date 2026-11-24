#![forbid(unsafe_code)]
use crate::deflater;
use crate::nifti::gzip::{adler32_update, crc32_update};
#[derive(Debug, Clone, Copy)]
pub enum PngSource<'a> {
    Gray(&'a [u8]),
    Rgba(&'a [u8]),
    Nrgba(&'a [u8]),
}
const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";
const IDAT_BUFFER: usize = 1 << 15;
fn write_chunk(out: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(name);
    out.extend_from_slice(data);
    let crc = crc32_update(crc32_update(0, name), data);
    out.extend_from_slice(&crc.to_be_bytes());
}
fn abs8(d: u8) -> i64 {
    if d < 128 {
        i64::from(d)
    } else {
        256 - i64::from(d)
    }
}
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let pc0 = i64::from(c);
    let pa = i64::from(b) - pc0;
    let pb = i64::from(a) - pc0;
    let pc = (pa + pb).abs();
    let (pa, pb) = (pa.abs(), pb.abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}
fn filter(cr: &mut [Vec<u8>; 5], pr: &[u8], bpp: usize) -> usize {
    let n = cr[0].len() - 1;
    let (c0, rest) = cr.split_at_mut(1);
    let cdat0 = &c0[0][1..];
    let pdat = &pr[1..];
    let [c1, c2, c3, c4] = rest else {
        return 0;
    };
    let (cdat1, cdat2, cdat3, cdat4) = (&mut c1[1..], &mut c2[1..], &mut c3[1..], &mut c4[1..]);
    let mut sum = 0;
    for i in 0..n {
        cdat2[i] = cdat0[i].wrapping_sub(pdat[i]);
        sum += abs8(cdat2[i]);
    }
    let mut best = sum;
    let mut chosen = 2;
    sum = 0;
    for i in 0..bpp.min(n) {
        cdat4[i] = cdat0[i].wrapping_sub(pdat[i]);
        sum += abs8(cdat4[i]);
    }
    for i in bpp..n {
        cdat4[i] = cdat0[i].wrapping_sub(paeth(cdat0[i - bpp], pdat[i], pdat[i - bpp]));
        sum += abs8(cdat4[i]);
        if sum >= best {
            break;
        }
    }
    if sum < best {
        best = sum;
        chosen = 4;
    }
    sum = 0;
    for v in cdat0.iter().take(n) {
        sum += abs8(*v);
        if sum >= best {
            break;
        }
    }
    if sum < best {
        best = sum;
        chosen = 0;
    }
    sum = 0;
    for i in 0..bpp.min(n) {
        cdat1[i] = cdat0[i];
        sum += abs8(cdat1[i]);
    }
    for i in bpp..n {
        cdat1[i] = cdat0[i].wrapping_sub(cdat0[i - bpp]);
        sum += abs8(cdat1[i]);
        if sum >= best {
            break;
        }
    }
    if sum < best {
        best = sum;
        chosen = 1;
    }
    sum = 0;
    for i in 0..bpp.min(n) {
        cdat3[i] = cdat0[i].wrapping_sub(pdat[i] / 2);
        sum += abs8(cdat3[i]);
    }
    for i in bpp..n {
        cdat3[i] =
            cdat0[i].wrapping_sub(((i64::from(cdat0[i - bpp]) + i64::from(pdat[i])) / 2) as u8);
        sum += abs8(cdat3[i]);
        if sum >= best {
            break;
        }
    }
    if sum < best {
        chosen = 3;
    }
    chosen
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PngError {
    InvalidSize { width: usize, height: usize },
    Deflate(String),
}
impl std::fmt::Display for PngError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PngError::InvalidSize { width, height } => {
                write!(
                    f,
                    "png: invalid format: invalid image size: {width}x{height}"
                )
            }
            PngError::Deflate(text) => f.write_str(text),
        }
    }
}
impl std::error::Error for PngError {}
pub fn encode_png(source: PngSource<'_>, width: usize, height: usize) -> Result<Vec<u8>, PngError> {
    if width == 0 || height == 0 {
        return Err(PngError::InvalidSize { width, height });
    }
    let (color_type, bpp) = match source {
        PngSource::Gray(_) => (0u8, 1usize),
        PngSource::Rgba(p) | PngSource::Nrgba(p) => {
            let opaque = p.as_chunks::<4>().0.iter().all(|px| px[3] == 0xff);
            if opaque { (2, 3) } else { (6, 4) }
        }
    };
    let mut out = Vec::new();
    out.extend_from_slice(PNG_HEADER);
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&(width as u32).to_be_bytes());
    ihdr[4..8].copy_from_slice(&(height as u32).to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = color_type;
    write_chunk(&mut out, b"IHDR", &ihdr);
    let sz = 1 + bpp * width;
    let mut cr: [Vec<u8>; 5] = std::array::from_fn(|i| {
        let mut v = vec![0u8; sz];
        v[0] = i as u8;
        v
    });
    let mut pr = vec![0u8; sz];
    let mut raw = Vec::with_capacity(height * sz);
    let mut ends = Vec::with_capacity(height);
    for y in 0..height {
        match source {
            PngSource::Gray(p) => cr[0][1..].copy_from_slice(&p[y * width..(y + 1) * width]),
            PngSource::Rgba(p) | PngSource::Nrgba(p) => {
                let row = &p[y * width * 4..(y + 1) * width * 4];
                let premultiplied = matches!(source, PngSource::Rgba(_));
                for (x, s) in row.as_chunks::<4>().0.iter().enumerate() {
                    if bpp == 3 {
                        cr[0][1 + 3 * x..4 + 3 * x].copy_from_slice(&s[..3]);
                        continue;
                    }
                    let d = &mut cr[0][1 + 4 * x..5 + 4 * x];
                    if !premultiplied || s[3] == 0xff {
                        d.copy_from_slice(s);
                    } else if s[3] == 0 {
                        d.copy_from_slice(&[0, 0, 0, 0]);
                    } else {
                        const M: u32 = 0x101 * 0xffff;
                        let a = u32::from(s[3]) * 0x101;
                        d[0] = ((u32::from(s[0]) * M / a) >> 8) as u8;
                        d[1] = ((u32::from(s[1]) * M / a) >> 8) as u8;
                        d[2] = ((u32::from(s[2]) * M / a) >> 8) as u8;
                        d[3] = s[3];
                    }
                }
            }
        }
        let f = filter(&mut cr, &pr, bpp);
        raw.extend_from_slice(&cr[f]);
        ends.push(raw.len());
        std::mem::swap(&mut pr, &mut cr[0]);
        cr[0][0] = 0;
    }
    let deflated = deflater::compress_chunks(&raw, &ends, deflater::DEFAULT_COMPRESSION)
        .map_err(PngError::Deflate)?;
    let mut zlib = Vec::with_capacity(deflated.len() + 6);
    zlib.extend_from_slice(&[0x78, 0x9c]);
    zlib.extend_from_slice(&deflated);
    zlib.extend_from_slice(&adler32_update(1, &raw).to_be_bytes());
    for chunk in zlib.chunks(IDAT_BUFFER) {
        write_chunk(&mut out, b"IDAT", chunk);
    }
    write_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}
struct IdatBuffer {
    buf: Vec<u8>,
    chunks: Vec<Vec<u8>>,
}
impl IdatBuffer {
    fn write(&mut self, mut p: &[u8]) {
        while p.len() > IDAT_BUFFER - self.buf.len() {
            if self.buf.is_empty() {
                self.chunks.push(p.to_vec());
                return;
            }
            let n = IDAT_BUFFER - self.buf.len();
            self.buf.extend_from_slice(&p[..n]);
            self.flush();
            p = &p[n..];
        }
        self.buf.extend_from_slice(p);
    }
    fn flush(&mut self) {
        if !self.buf.is_empty() {
            self.chunks.push(std::mem::take(&mut self.buf));
        }
    }
}
pub(crate) fn encode_nrgba64_no_compression(
    pix: &[u16],
    width: usize,
    height: usize,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || pix.len() != width * height * 4 {
        return Err(format!(
            "png: invalid format: invalid image size: {width}x{height}"
        ));
    }
    let opaque = pix.as_chunks::<4>().0.iter().all(|p| p[3] == 0xffff);
    let (color_type, channels) = if opaque { (2u8, 3usize) } else { (6, 4) };
    let mut out = Vec::new();
    out.extend_from_slice(PNG_HEADER);
    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&(width as u32).to_be_bytes());
    ihdr[4..8].copy_from_slice(&(height as u32).to_be_bytes());
    ihdr[8] = 16;
    ihdr[9] = color_type;
    write_chunk(&mut out, b"IHDR", &ihdr);
    let row_len = 1 + 2 * channels * width;
    let mut raw = Vec::with_capacity(height * row_len);
    for row in pix.chunks_exact(width * 4) {
        raw.push(0);
        for p in row.as_chunks::<4>().0 {
            for v in &p[..channels] {
                raw.extend_from_slice(&v.to_be_bytes());
            }
        }
    }
    const STORED: usize = 65535;
    let mut idat = IdatBuffer {
        buf: Vec::with_capacity(IDAT_BUFFER),
        chunks: Vec::new(),
    };
    idat.write(&[0x78, 0x01]);
    for block in raw.chunks(STORED) {
        let n = block.len() as u16;
        idat.write(&[0x00]);
        let mut header = [0u8; 4];
        header[0..2].copy_from_slice(&n.to_le_bytes());
        header[2..4].copy_from_slice(&(!n).to_le_bytes());
        idat.write(&header);
        idat.write(block);
    }
    idat.write(&[0x01]);
    idat.write(&[0x00, 0x00, 0xff, 0xff]);
    idat.write(&adler32_update(1, &raw).to_be_bytes());
    idat.flush();
    for chunk in &idat.chunks {
        write_chunk(&mut out, b"IDAT", chunk);
    }
    write_chunk(&mut out, b"IEND", &[]);
    Ok(out)
}
