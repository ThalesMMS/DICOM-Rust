use super::{Error, Kind};
pub(crate) const MARKER_SOF0: u8 = 0xc0;
pub(crate) const MARKER_SOF1: u8 = 0xc1;
pub(crate) const MARKER_SOF2: u8 = 0xc2;
pub(crate) const MARKER_SOF3: u8 = 0xc3;
const MARKER_DHT: u8 = 0xc4;
const MARKER_JPG: u8 = 0xc8;
const MARKER_DAC: u8 = 0xcc;
const MARKER_SOI: u8 = 0xd8;
const MARKER_EOI: u8 = 0xd9;
const MARKER_SOS: u8 = 0xda;
pub(crate) const MARKER_APP0: u8 = 0xe0;
pub(crate) const MARKER_APP2: u8 = 0xe2;
pub(crate) const MARKER_APPE: u8 = 0xee;
pub(crate) const MARKER_COM: u8 = 0xfe;
const MARKER_TEM: u8 = 0x01;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameComponent {
    pub id: u8,
    pub horizontal: i64,
    pub vertical: i64,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct StreamSegment {
    pub marker: u8,
    pub start: usize,
    pub end: usize,
}
impl StreamSegment {
    pub fn payload<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        if self.end - self.start < 4 {
            return &[];
        }
        &data[self.start + 4..self.end]
    }
}
#[derive(Clone, Debug, Default)]
pub(crate) struct JpegStream {
    pub sof_marker: u8,
    pub sof_count: usize,
    pub precision: i64,
    pub rows: i64,
    pub columns: i64,
    pub components: Vec<FrameComponent>,
    pub predictors: Vec<i64>,
    pub jfif: bool,
    pub adobe_transform: Option<u8>,
    pub segments: Vec<StreamSegment>,
    pub eoi: usize,
}
fn be16(b: &[u8]) -> usize {
    usize::from(b[0]) << 8 | usize::from(b[1])
}
pub(crate) fn parse_jpeg_stream(data: &[u8]) -> Result<JpegStream, Error> {
    parse_jpeg_stream_limited(data, 0)
}
pub(crate) fn parse_jpeg_stream_limited(
    data: &[u8],
    max_segments: usize,
) -> Result<JpegStream, Error> {
    let mut s = JpegStream::default();
    if data.len() < 4 || data[0] != 0xff || data[1] != MARKER_SOI {
        return Err(Error::bare(Kind::NotJpeg));
    }
    s.segments.push(StreamSegment {
        marker: MARKER_SOI,
        start: 0,
        end: 2,
    });
    let mut i = 2usize;
    loop {
        if max_segments > 0 && s.segments.len() >= max_segments {
            return Err(Error::bare(Kind::ImportLimit));
        }
        if i + 2 > data.len() {
            return Err(Error::new(Kind::TruncatedJpeg, "no EOI marker"));
        }
        if data[i] != 0xff {
            return Err(Error::new(
                Kind::MalformedJpeg,
                format!("expected a marker at offset {i}"),
            ));
        }
        let marker = data[i + 1];
        if marker == 0xff {
            i += 1;
            continue;
        }
        if marker == MARKER_EOI {
            s.segments.push(StreamSegment {
                marker,
                start: i,
                end: i + 2,
            });
            s.eoi = i + 2;
            if s.sof_count == 0 {
                return Err(Error::new(Kind::MalformedJpeg, "no frame header"));
            }
            return Ok(s);
        }
        if marker == MARKER_SOI {
            return Err(Error::new(Kind::MalformedJpeg, "SOI inside the stream"));
        }
        if marker == MARKER_TEM || (0xd0..=0xd7).contains(&marker) || marker == 0x00 {
            return Err(Error::new(
                Kind::MalformedJpeg,
                format!("marker FF{marker:02X} outside a scan"),
            ));
        }
        if i + 4 > data.len() {
            return Err(Error::new(
                Kind::TruncatedJpeg,
                format!("marker FF{marker:02X} without a length"),
            ));
        }
        let length = be16(&data[i + 2..]);
        let end = i + 2 + length;
        if length < 2 || end > data.len() {
            return Err(Error::new(
                Kind::TruncatedJpeg,
                format!("marker FF{marker:02X} segment of {length} bytes"),
            ));
        }
        let mut segment = StreamSegment {
            marker,
            start: i,
            end,
        };
        s.inspect(&segment, data)?;
        if marker == MARKER_SOS {
            segment.end = scan_end(data, end);
            if segment.end == data.len() {
                return Err(Error::new(
                    Kind::TruncatedJpeg,
                    "scan without a following marker",
                ));
            }
        }
        s.segments.push(segment);
        i = segment.end;
    }
}
fn scan_end(data: &[u8], mut i: usize) -> usize {
    loop {
        let Some(j) = data[i..].iter().position(|&b| b == 0xff) else {
            return data.len();
        };
        i += j;
        let mut k = i + 1;
        while k < data.len() && data[k] == 0xff {
            k += 1;
        }
        if k >= data.len() {
            return data.len();
        }
        let next = data[k];
        if next == 0x00 || (0xd0..=0xd7).contains(&next) {
            i = k + 1;
            continue;
        }
        return k - 1;
    }
}
fn is_sof(marker: u8) -> bool {
    (0xc0..=0xcf).contains(&marker)
        && marker != MARKER_DHT
        && marker != MARKER_JPG
        && marker != MARKER_DAC
}
impl JpegStream {
    fn inspect(&mut self, segment: &StreamSegment, data: &[u8]) -> Result<(), Error> {
        let payload = segment.payload(data);
        let marker = segment.marker;
        if is_sof(marker) {
            self.sof_count += 1;
            if self.sof_count > 1 {
                return Err(Error::new(
                    Kind::UnsupportedJpegProcess,
                    "more than one frame header",
                ));
            }
            if payload.len() < 6 {
                return Err(Error::new(Kind::MalformedJpeg, "short frame header"));
            }
            self.sof_marker = marker;
            self.precision = i64::from(payload[0]);
            self.rows = be16(&payload[1..]) as i64;
            self.columns = be16(&payload[3..]) as i64;
            let count = usize::from(payload[5]);
            if payload.len() < 6 + 3 * count {
                return Err(Error::new(
                    Kind::MalformedJpeg,
                    format!("frame header declares {count} components"),
                ));
            }
            for c in 0..count {
                let sampling = payload[6 + 3 * c + 1];
                self.components.push(FrameComponent {
                    id: payload[6 + 3 * c],
                    horizontal: i64::from(sampling >> 4),
                    vertical: i64::from(sampling & 0x0f),
                });
            }
        } else if marker == MARKER_SOS {
            if self.sof_count == 0 {
                return Err(Error::new(
                    Kind::MalformedJpeg,
                    "scan before the frame header",
                ));
            }
            if payload.is_empty() || payload.len() < 1 + 2 * usize::from(payload[0]) + 3 {
                return Err(Error::new(Kind::MalformedJpeg, "short scan header"));
            }
            self.predictors
                .push(i64::from(payload[1 + 2 * usize::from(payload[0])]));
        } else if marker == MARKER_APP0 {
            if payload.starts_with(b"JFIF\x00") {
                self.jfif = true;
            }
        } else if marker == MARKER_APPE && payload.len() >= 12 && payload.starts_with(b"Adobe") {
            self.adobe_transform = Some(payload[11]);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scans_end_at_markers() {
        let data = [0x12, 0xff, 0x00, 0xff, 0xd0, 0x34, 0xff, 0xff, 0xd9];
        assert_eq!(scan_end(&data, 0), 7);
        assert_eq!(scan_end(&[1, 2, 0xff], 0), 3);
    }
    #[test]
    fn rejects_non_jpeg() {
        assert!(parse_jpeg_stream(b"\x89PNG").unwrap_err().is(Kind::NotJpeg));
        let err = parse_jpeg_stream(b"\xff\xd8\xff\xd9").unwrap_err();
        assert_eq!(
            err.to_string(),
            "encapimage: malformed JPEG stream: no frame header"
        );
    }
}
