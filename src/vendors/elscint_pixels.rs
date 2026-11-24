use super::{Error, Kind, Limits, private_value, trim_right_bytes};
use crate::dictionary::reservations::private_tag;
use crate::object::{Object, Tag};
use crate::pixel::Metadata;
use crate::pixel::pixeldata::{extract_metadata, frame_size};
pub const ELSCINT_PMSCT_RLE1: &str = "PMSCT_RLE1";
pub const ELSCINT_PMSCT_RGB1: &str = "PMSCT_RGB1";
pub const DIAGNOSTIC_ELSCINT_PRIVATE_PIXELS: &str = "elscint-private-pixel-data";
pub const DIAGNOSTIC_ELSCINT_STORED: &str = "elscint-private-pixel-data-stored";
pub const DIAGNOSTIC_ELSCINT_STANDARD_PIXEL_DATA: &str = "elscint-compressed-standard-pixel-data";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElscintPixels {
    pub compression: String,
    pub source: Tag,
    pub metadata: Metadata,
    pub frame: Vec<u8>,
    pub diagnostics: Vec<&'static str>,
}
const ELSCINT_GROUP: u16 = 0x07A1;
const ELSCINT_COMPRESSION: u8 = 0x11;
const ELSCINT_PIXEL_ELEMENT: u8 = 0x0A;
pub fn elscint_pixel_data_of(dataset: &Object, limits: Limits) -> Result<ElscintPixels, Error> {
    let limits = limits.normalized();
    let not_found = || Error::bare(Kind::NotFound);
    let compression_tag = private_tag(dataset, ELSCINT_GROUP, "ELSCINT1", ELSCINT_COMPRESSION)
        .map_err(|_| not_found())?;
    if !dataset.has(compression_tag) {
        return Err(not_found());
    }
    let compression = dataset
        .lookup_string(compression_tag)
        .map_err(|err| Error::new(Kind::Invalid, format!("ELSCINT1 compression type: {err}")))?;
    let compression = trim_right_bytes(compression.as_bytes(), b"\x00 ");
    let compression = String::from_utf8_lossy(compression).trim().to_string();
    if compression != ELSCINT_PMSCT_RLE1 && compression != ELSCINT_PMSCT_RGB1 {
        return Err(Error::new(
            Kind::Invalid,
            format!(
                "ELSCINT1 compression {}",
                crate::textfmt::quote(&compression)
            ),
        ));
    }
    let metadata = extract_metadata(dataset).map_err(|err| Error::new(Kind::Invalid, err))?;
    check_elscint_metadata(&compression, &metadata)?;
    let (rows, columns) = (i64::from(metadata.rows), i64::from(metadata.columns));
    let frame_bytes = frame_size(&metadata);
    if frame_bytes > limits.max_decoded_bytes {
        return Err(Error::new(Kind::Limit, format!("{frame_bytes}-byte frame")));
    }
    let mut diagnostics = vec![DIAGNOSTIC_ELSCINT_PRIVATE_PIXELS];
    let (data, source) = match private_value(
        dataset,
        ELSCINT_GROUP,
        &["ELSCINT1"],
        ELSCINT_PIXEL_ELEMENT,
        limits,
    ) {
        Ok(found) => found,
        Err(err) if err.is(Kind::NotFound) => {
            let raw = dataset
                .get_raw(Tag::PIXEL_DATA)
                .filter(|raw| raw.len() as i64 != frame_bytes)
                .ok_or_else(not_found)?;
            if raw.len() as i64 > limits.max_bytes {
                return Err(Error::new(Kind::Limit, format!("{} bytes", raw.len())));
            }
            diagnostics.push(DIAGNOSTIC_ELSCINT_STANDARD_PIXEL_DATA);
            (raw, Tag::PIXEL_DATA)
        }
        Err(err) => return Err(err),
    };
    let frame = if data.len() as i64 == frame_bytes {
        diagnostics.push(DIAGNOSTIC_ELSCINT_STORED);
        data
    } else if compression == ELSCINT_PMSCT_RLE1 {
        decode_elscint_pmsct_rle1(&data, rows, columns)?
    } else {
        let planar = metadata.planar_configuration_present && metadata.planar_configuration == 1;
        decode_elscint_pmsct_rgb1(&data, rows, columns, planar)?
    };
    Ok(ElscintPixels {
        compression,
        source,
        metadata,
        frame,
        diagnostics,
    })
}
fn check_elscint_metadata(compression: &str, m: &Metadata) -> Result<(), Error> {
    if m.rows == 0 || m.columns == 0 || m.number_of_frames > 1 {
        return Err(Error::new(
            Kind::Invalid,
            format!("{compression} needs one frame of known size"),
        ));
    }
    let (samples, bits, what) = if compression == ELSCINT_PMSCT_RLE1 {
        (1, 16, "16-bit monochrome")
    } else {
        (3, 8, "8-bit RGB")
    };
    if m.samples_per_pixel != samples || m.bits_allocated != bits {
        return Err(Error::new(
            Kind::Invalid,
            format!(
                "{compression} needs {what} samples, not {} x {} bits",
                m.samples_per_pixel, m.bits_allocated
            ),
        ));
    }
    Ok(())
}
struct RunReader<'a> {
    data: &'a [u8],
    pos: usize,
    repeat: usize,
    value: u8,
}
impl RunReader<'_> {
    fn next(&mut self) -> Result<Option<u8>, Error> {
        if self.repeat > 0 {
            self.repeat -= 1;
            return Ok(Some(self.value));
        }
        let Some(&b) = self.data.get(self.pos) else {
            return Ok(None);
        };
        self.pos += 1;
        if b != 0xA5 {
            return Ok(Some(b));
        }
        if self.pos + 2 > self.data.len() {
            return Err(Error::new(
                Kind::Invalid,
                format!("PMSCT_RLE1 run truncated at byte {}", self.pos - 1),
            ));
        }
        self.repeat = usize::from(self.data[self.pos]);
        self.value = self.data[self.pos + 1];
        self.pos += 2;
        Ok(Some(self.value))
    }
}
pub fn decode_elscint_pmsct_rle1(data: &[u8], rows: i64, columns: i64) -> Result<Vec<u8>, Error> {
    if rows <= 0 || columns <= 0 {
        return Err(Error::new(
            Kind::Invalid,
            format!("PMSCT_RLE1 frame {columns}x{rows}"),
        ));
    }
    let want = (rows * columns) as usize;
    let mut out = vec![0u8; 2 * want];
    let mut expanded = RunReader {
        data,
        pos: 0,
        repeat: 0,
        value: 0,
    };
    let mut sample = 0u16;
    let mut count = 0usize;
    while let Some(b) = expanded.next()? {
        if b == 0x5A {
            let (lo, hi) = (expanded.next(), expanded.next());
            let (Ok(Some(lo)), Ok(Some(hi))) = (lo, hi) else {
                return Err(Error::new(
                    Kind::Invalid,
                    "PMSCT_RLE1 absolute sample truncated",
                ));
            };
            sample = u16::from(lo) | u16::from(hi) << 8;
        } else {
            sample = sample.wrapping_add(b as i8 as i16 as u16);
        }
        if count == want + 1 {
            return Err(Error::new(
                Kind::Invalid,
                format!("PMSCT_RLE1 stream holds more than {want} samples"),
            ));
        }
        if count < want {
            out[2 * count..2 * count + 2].copy_from_slice(&sample.to_le_bytes());
        }
        count += 1;
    }
    if count < want {
        return Err(Error::new(
            Kind::Invalid,
            format!("PMSCT_RLE1 stream holds {count} of {want} samples"),
        ));
    }
    Ok(out)
}
pub fn decode_elscint_pmsct_rgb1(
    data: &[u8],
    rows: i64,
    columns: i64,
    planar: bool,
) -> Result<Vec<u8>, Error> {
    if rows <= 0 || columns <= 0 {
        return Err(Error::new(
            Kind::Invalid,
            format!("PMSCT_RGB1 frame {columns}x{rows}"),
        ));
    }
    const TOGGLE: u8 = 0x81;
    const ESCAPE: u8 = 0x82;
    const REPEAT: u8 = 0x83;
    let pixels = (rows * columns) as usize;
    let mut out = vec![0u8; 3 * pixels];
    let mut put = |p: usize, rgb: [u8; 3]| {
        if planar {
            out[p] = rgb[0];
            out[pixels + p] = rgb[1];
            out[2 * pixels + p] = rgb[2];
        } else {
            out[3 * p..3 * p + 3].copy_from_slice(&rgb);
        }
    };
    let mut pos = 0usize;
    let mut take = |n: usize| -> Result<&[u8], Error> {
        let start = pos;
        if start + n > data.len() {
            return Err(Error::new(
                Kind::Invalid,
                format!("PMSCT_RGB1 stream truncated at byte {start}"),
            ));
        }
        pos += n;
        Ok(&data[start..start + n])
    };
    let mut gray = true;
    let mut value = [0u8; 3];
    let add = |value: &mut [u8; 3], gray: bool, differences: &[u8]| {
        if gray {
            value[0] = value[0].wrapping_add(differences[0]);
            (value[1], value[2]) = (value[0], value[0]);
        } else {
            for (v, d) in value.iter_mut().zip(differences) {
                *v = v.wrapping_add(*d);
            }
        }
    };
    let mut p = 0usize;
    while p < pixels {
        let code = take(1)?[0];
        match code {
            TOGGLE => {
                gray = !gray;
                value = [0; 3];
            }
            REPEAT => {
                let n = usize::from(take(1)?[0]);
                if n > pixels - p {
                    return Err(Error::new(
                        Kind::Invalid,
                        format!(
                            "PMSCT_RGB1 repeat of {n} exceeds the {} remaining pixels",
                            pixels - p
                        ),
                    ));
                }
                for _ in 0..n {
                    put(p, value);
                    p += 1;
                }
            }
            ESCAPE => {
                let differences = take(if gray { 1 } else { 3 })?;
                add(&mut value, gray, differences);
                put(p, value);
                p += 1;
            }
            _ => {
                let mut differences = [code, 0, 0];
                if !gray {
                    differences[1..].copy_from_slice(take(2)?);
                }
                add(&mut value, gray, &differences);
                put(p, value);
                p += 1;
            }
        }
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rle1() {
        let out = decode_elscint_pmsct_rle1(&[0x5A, 0xE8, 0x03, 0x01, 0xA5, 0x01, 0x00], 2, 2);
        assert_eq!(
            out.unwrap(),
            [0xE8, 0x03, 0xE9, 0x03, 0xE9, 0x03, 0xE9, 0x03]
        );
        let err = decode_elscint_pmsct_rle1(&[1], 1, 2).unwrap_err();
        assert_eq!(
            err.to_string(),
            "dicom/vendors: invalid private blob: PMSCT_RLE1 stream holds 1 of 2 samples"
        );
    }
    #[test]
    fn rgb1() {
        let out = decode_elscint_pmsct_rgb1(&[5, 0x83, 1, 0x81, 1, 2, 3], 1, 3, false).unwrap();
        assert_eq!(out, [5, 5, 5, 5, 5, 5, 1, 2, 3]);
        let planar = decode_elscint_pmsct_rgb1(&[0x81, 1, 2, 3], 1, 1, true).unwrap();
        assert_eq!(planar, [1, 2, 3]);
    }
}
